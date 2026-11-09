//! Integration regression sources, not execution evidence.
use super::*;

fn session() -> DocumentSession {
    DocumentSession::new(web_html::parse("<p id=result>safe</p>").unwrap())
}

fn text(session: &mut DocumentSession) -> String {
    let id = session.query_selector("#result").unwrap().unwrap();
    session.document.text_content(id)
}

fn install_identity(session: &mut DocumentSession) {
    // Binary module: (func (export "id") (param i64) (result i64) local.get 0).
    session
        .execute(
            "const bytes=new Uint8Array([0,97,115,109,1,0,0,0,1,6,1,96,1,126,1,126,3,2,1,0,7,6,1,2,105,100,0,0,10,6,1,4,0,32,0,11]);const instance=new WebAssembly.Instance(new WebAssembly.Module(bytes));",
        )
        .unwrap();
}

#[test]
fn i64_identity_writes_exact_bigint_and_signed_wrap_to_dom() {
    let mut session = session();
    install_identity(&mut session);
    session
        .execute(
            r#"
            bytes[0] = 255;
            const exact = instance.exports.id(9007199254740993n);
            document.getElementById('result').textContent = typeof exact + ':' + exact + ':' +
                instance.exports.id('9007199254740993') + ':' +
                instance.exports.id(18446744073709551615n) + ':' +
                instance.exports.id(9223372036854775808n) + ':' +
                instance.exports.id(true) + ':' + instance.exports.id(false);
            "#,
        )
        .unwrap();
    assert_eq!(
        text(&mut session),
        "bigint:9007199254740993:9007199254740993:-1:-9223372036854775808:1:0"
    );
    assert!(!session.scripts_disabled);
}

#[test]
fn i64_coercion_observes_number_hint_and_keeps_ordinary_error_dom_effects() {
    let mut session = session();
    install_identity(&mut session);
    session
        .execute(
            r#"
            let calls = 0;
            let hint = '';
            const value = {};
            value[Symbol.toPrimitive] = function(h) {
                calls++;
                hint = h;
                return '9007199254740993';
            };
            const exact = instance.exports.id(value);
            let rejected = '';
            try { instance.exports.id(42); }
            catch (error) { rejected = error.name; }
            const bad = {};
            bad.valueOf = function() {
                document.getElementById('result').textContent = 'effect';
                return 7;
            };
            let badName = '';
            try { instance.exports.id(bad); }
            catch (error) { badName = error.name; }
            document.getElementById('result').textContent += ':' + calls + ':' + hint + ':' +
                exact + ':' + rejected + ':' + badName;
            "#,
        )
        .unwrap();
    assert_eq!(
        text(&mut session),
        "effect:1:number:9007199254740993:TypeError:TypeError"
    );
    assert!(!session.scripts_disabled);
}

#[test]
fn i64_coercion_resource_failure_rolls_back_dom_and_vm_and_disables_scripts() {
    let mut session = session();
    session.runtime = Runtime::new(web_javascript::Limits {
        instructions: 2000,
        ..web_javascript::Limits::default()
    });
    install_identity(&mut session);
    session.execute("var changed=0;").unwrap();
    let error = session
        .execute(
            r#"
            const value = {};
            value.valueOf = function() {
                document.getElementById('result').textContent = 'bad';
                while (true) { changed++; }
            };
            try { instance.exports.id(value); }
            catch (error) { document.getElementById('result').textContent = 'caught'; }
            "#,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(text(&mut session), "safe");
    assert_eq!(
        session.runtime.global_value("changed"),
        Some(Value::Number(0.))
    );
    assert!(session.scripts_disabled);
    assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
}
