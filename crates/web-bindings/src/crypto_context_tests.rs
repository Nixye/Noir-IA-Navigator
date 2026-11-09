//! Regression sources for the conservative, loader-authorized HTTPS policy.
use super::*;

fn session(address: Option<&str>) -> DocumentSession {
    let mut session = DocumentSession::new(web_html::parse("<p id=x>safe</p>").unwrap());
    if let Some(address) = address {
        session.set_document_url(address).unwrap();
    }
    session
}

fn allowed(session: &mut DocumentSession) -> bool {
    DomHost {
        document: &mut session.document,
        state: &mut session.state,
    }
    .allows_subtle_crypto()
}

#[test]
fn only_trusted_https_document_metadata_allows_native_crypto() {
    for address in [
        None,
        Some("http://example.test/page"),
        Some("http://localhost/page"),
        Some("file:///page.html"),
        Some("noir://home"),
        Some("data:text/html,hello"),
    ] {
        let mut session = session(address);
        assert!(!allowed(&mut session));
        assert_eq!(
            session
                .execute("typeof crypto + ':' + typeof window.crypto;")
                .unwrap(),
            Value::string("undefined:undefined")
        );
        assert_eq!(
            session.execute("var crypto={subtle:{digest:17}};crypto=23;window.crypto={subtle:{digest:19}};typeof window.crypto.subtle.digest;").unwrap(),
            Value::string("number")
        );
        assert!(!allowed(&mut session));
        assert!(session.set_document_url("https://example.test/").is_err());
    }
    let mut session = session(Some("https://example.test/page"));
    assert!(allowed(&mut session));
    assert_eq!(
        session
            .execute(
                "crypto===window.crypto && crypto===self.crypto && crypto===globalThis.crypto;"
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        session.execute("{let crypto=7;document.getElementById('x').textContent=crypto+':'+typeof window.crypto.subtle.digest;}document.getElementById('x').textContent;").unwrap(),
        Value::string("7:function")
    );
}

#[test]
fn https_digest_fulfills_with_real_sha256_bytes_in_a_microtask() {
    let mut session = session(Some("https://example.test/"));
    session.execute("crypto.subtle.digest('SHA-256',new Uint8Array([97,98,99])).then(function(buffer){const bytes=new Uint8Array(buffer);let hex='';for(let i=0;i<bytes.length;i++){hex+='0123456789abcdef'[bytes[i]>>4]+'0123456789abcdef'[bytes[i]&15];}document.getElementById('x').textContent=hex;});").unwrap();
    let id = session.query_selector("#x").unwrap().unwrap();
    assert_eq!(
        session.document.text_content(id),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn url_freezes_before_every_execution_entry_even_when_it_fails() {
    for entry in 0..7 {
        let mut session = session(Some("http://example.test/original"));
        match entry {
            0 => {
                session.execute("1;").unwrap();
            }
            1 => {
                assert!(session.execute("let = ;").is_err());
            }
            2 => {
                assert!(session.dispatch_event(usize::MAX, "click").is_err());
            }
            3 => {
                session.run_due_timers(0).unwrap();
            }
            4 => {
                session.run_initial(&[]);
            }
            5 => {
                assert!(session.execute_registered_module("missing").is_err());
            }
            _ => {
                let id = session.query_selector("#x").unwrap().unwrap();
                session.dispatch_event(id, "click").unwrap();
            }
        }
        assert!(
            session
                .set_document_url("https://example.test/upgraded")
                .is_err()
        );
        assert!(!allowed(&mut session));
        assert_eq!(
            session.state.document_url.as_ref().unwrap().as_str(),
            "http://example.test/original"
        );
    }
}

#[test]
fn trusted_url_can_be_replaced_only_before_execution_and_survives_setup() {
    let mut session = session(Some("http://example.test/"));
    session.set_document_url("https://example.test/").unwrap();
    session
        .set_session_storage(browser_ipc::SessionStorageSeed::default())
        .unwrap();
    session.set_audio_metadata(vec![]).unwrap();
    assert!(allowed(&mut session));
    session.run_initial(&[]);
    assert!(session.set_document_url("http://example.test/").is_err());
}

#[test]
fn source_url_comments_and_location_assignments_cannot_grant_crypto() {
    let mut session = session(Some("http://example.test/"));
    assert_eq!(
        session
            .execute("//# sourceURL=https://example.test/script.js\ntypeof crypto;")
            .unwrap(),
        Value::string("undefined")
    );
    assert!(
        session
            .execute("window.location={href:'https://example.test/'};")
            .is_err()
    );
    assert!(!allowed(&mut session));
    assert_eq!(
        session.execute("typeof window.crypto;").unwrap(),
        Value::string("undefined")
    );
    assert!(session.set_document_url("https://example.test/").is_err());
}

#[test]
fn crypto_does_not_displace_storage_or_event_accessors() {
    let mut session = session(Some("https://example.test/"));
    session
        .set_session_storage(browser_ipc::SessionStorageSeed {
            access: browser_ipc::SessionStorageAccess::ReadWrite,
            entries: vec![],
        })
        .unwrap();
    session.execute("sessionStorage.setItem('k','stored');onload=function(){document.getElementById('x').textContent=sessionStorage.getItem('k')+':'+(crypto===window.crypto);};").unwrap();
    session.run_initial(&[]);
    let id = session.query_selector("#x").unwrap().unwrap();
    assert_eq!(session.document.text_content(id), "stored:true");
}

#[test]
fn digest_resource_failure_rolls_back_dom_and_vm_without_unfreezing_url() {
    let mut session = session(Some("https://example.test/"));
    session.runtime = Runtime::new(web_javascript::Limits {
        instructions: 2000,
        ..web_javascript::Limits::default()
    });
    session
        .execute("var changed=0;var bytes=new ArrayBuffer(65536);")
        .unwrap();
    let error = session.execute("changed=1;document.getElementById('x').textContent='bad';try{crypto.subtle.digest('SHA-512',bytes);}catch(e){document.getElementById('x').textContent='caught';}").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    let id = session.query_selector("#x").unwrap().unwrap();
    assert_eq!(session.document.text_content(id), "safe");
    assert_eq!(
        session.runtime.global_value("changed"),
        Some(Value::Number(0.))
    );
    assert!(session.scripts_disabled);
    assert!(session.set_document_url("http://example.test/").is_err());
    assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
}
