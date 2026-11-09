use super::*;

fn session() -> DocumentSession {
    let mut session =
        DocumentSession::new(web_html::parse("<body><p id=out>pending</p></body>").unwrap());
    session
        .set_document_url("https://example.test/dir/page.html")
        .unwrap();
    session
}
fn response(id: u64, source: Option<&str>) -> DynamicScriptResponse {
    DynamicScriptResponse {
        id,
        source: source.map(str::to_owned),
    }
}
fn check(session: &mut DocumentSession, source: &str) {
    assert_eq!(session.execute(source).unwrap(), Value::Bool(true));
}

#[test]
fn loads_external_source_only_on_correlated_completion_and_fires_load() {
    let mut s = session();
    s.execute("var events='';var script=document.createElement('script');script.src='next.js';script.onload=function(e){events+='load:'+e.bubbles+':'+(this===script);};document.head.appendChild(script);").unwrap();
    check(
        &mut s,
        "events==='' && script.src==='https://example.test/dir/next.js' && script.async===true && document.currentScript===null;",
    );
    let requests = s.take_script_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].src, "next.js");
    assert!(s.take_script_requests().is_empty());
    let report = s.complete_scripts(&[response(requests[0].id, Some("document.getElementById('out').textContent='loaded';var current=document.currentScript===script;"))]).unwrap();
    assert_eq!(report.executed, 1);
    assert!(report.warnings.is_empty());
    check(
        &mut s,
        "events==='load:false:true' && current && document.currentScript===null && document.getElementById('out').textContent==='loaded';",
    );
}

#[test]
fn source_after_insertion_and_subtree_insertion_queue_once_in_order() {
    let mut s = session();
    s.execute("var a=document.createElement('script');a.async=false;document.body.appendChild(a);a.setAttribute('src','one.js');a.src='changed.js';a.remove();document.head.appendChild(a);var div=document.createElement('div');var b=document.createElement('script');b.src='two.js';div.appendChild(b);document.body.appendChild(div);").unwrap();
    let requests = s.take_script_requests();
    assert_eq!(
        requests.iter().map(|r| r.src.as_str()).collect::<Vec<_>>(),
        ["one.js", "two.js"]
    );
    s.complete_scripts(&[
        response(requests[0].id, Some("var order='one';")),
        response(requests[1].id, Some("order+='two';")),
    ])
    .unwrap();
    check(&mut s, "order==='onetwo' && a.async===false;");
    assert!(s.take_script_requests().is_empty());
}

#[test]
fn detached_load_and_error_callbacks_work_without_refetching_changed_src() {
    let mut s = session();
    s.execute("var result='';var a=document.createElement('script');a.src='missing.js';a.onerror=function(e){result='error:'+e.cancelable;};document.body.appendChild(a);a.remove();a.src='different.js';").unwrap();
    let requests = s.take_script_requests();
    assert_eq!(requests[0].src, "missing.js");
    s.complete_scripts(&[response(requests[0].id, None)])
        .unwrap();
    check(&mut s, "result==='error:false';");
    assert!(s.take_script_requests().is_empty());
}

#[test]
fn next_load_from_completion_callback_forms_a_new_batch() {
    let mut s = session();
    s.execute("var a=document.createElement('script');a.src='a.js';a.onload=function(){var b=document.createElement('script');b.src='b.js';document.body.appendChild(b);};document.head.appendChild(a);").unwrap();
    let requests = s.take_script_requests();
    s.complete_scripts(&[response(requests[0].id, Some("var count=1;"))])
        .unwrap();
    let next = s.take_script_requests();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].src, "b.js");
    assert_ne!(requests[0].id, next[0].id);
    s.complete_scripts(&[response(next[0].id, Some("count++;"))])
        .unwrap();
    check(&mut s, "count===2;");
}

#[test]
fn parser_inner_html_and_started_clones_are_inert_but_fresh_clones_load() {
    let mut s = session();
    s.execute("document.body.innerHTML='<script id=p src=parser.js></script>';var parser=document.getElementById('p');parser.remove();document.head.appendChild(parser);var a=document.createElement('script');a.src='a.js';var b=a.cloneNode(true);document.body.appendChild(a);var c=a.cloneNode(true);document.body.appendChild(c);document.body.appendChild(b);").unwrap();
    let requests = s.take_script_requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.src == "a.js"));
}

#[test]
fn inactive_and_disconnected_subtrees_do_not_request_until_moved_out() {
    let mut s = session();
    s.execute("var t=document.createElement('template');var a=document.createElement('script');a.src='a.js';t.appendChild(a);document.body.appendChild(t);").unwrap();
    assert!(s.take_script_requests().is_empty());
    s.execute("document.head.appendChild(a);").unwrap();
    assert_eq!(s.take_script_requests().len(), 1);
}

#[test]
fn invalid_and_replayed_or_reordered_completions_never_execute_sources() {
    let mut s = session();
    s.execute("var result=0;for(var i=0;i<2;i++){var a=document.createElement('script');a.src='a.js';document.head.appendChild(a);}").unwrap();
    let requests = s.take_script_requests();
    let bad = [
        response(requests[1].id, Some("result=9;")),
        response(requests[0].id, Some("result=9;")),
    ];
    assert!(s.complete_scripts(&bad).is_err());
    assert!(
        s.complete_scripts(&[response(requests[0].id, Some("result=9;"))])
            .is_err()
    );
    check(&mut s, "result===0;");
    let good = [
        response(requests[0].id, Some("result++;")),
        response(requests[1].id, Some("result++;")),
    ];
    s.complete_scripts(&good).unwrap();
    assert!(s.complete_scripts(&good).is_err());
    check(&mut s, "result===2;");
}

#[test]
fn ordinary_runtime_exception_still_fires_load_and_later_scripts_continue() {
    let mut s = session();
    s.execute("var events='';window.onerror=function(){events+='window;';};var a=document.createElement('script');a.src='a.js';a.onload=function(){events+='load;';};a.onerror=function(){events+='bad;';};document.head.appendChild(a);").unwrap();
    let requests = s.take_script_requests();
    let report = s
        .complete_scripts(&[response(
            requests[0].id,
            Some("events+='effect;';throw 'ordinary';"),
        )])
        .unwrap();
    assert_eq!(report.warnings.len(), 1);
    check(&mut s, "events==='effect;window;load;';");
}

#[test]
fn resource_exhaustion_rolls_back_dom_and_discards_all_requests() {
    let mut s = session();
    s.execute("var a=document.createElement('script');a.src='a.js';document.head.appendChild(a);")
        .unwrap();
    let requests = s.take_script_requests();
    let report = s.complete_scripts(&[response(requests[0].id, Some("document.getElementById('out').textContent='bad';var b=document.createElement('script');b.src='b.js';document.head.appendChild(b);while(true){}"))]).unwrap();
    assert_eq!(report.warnings.len(), 1);
    assert!(s.scripts_disabled);
    assert!(s.take_script_requests().is_empty());
    let out = s
        .document
        .elements()
        .find(|(_, e)| e.attr("id") == Some("out"))
        .unwrap()
        .0;
    assert_eq!(s.document.text_content(out), "pending");
}

#[test]
fn count_and_source_quotas_disable_scripts_without_partial_evaluation() {
    let mut s = session();
    let error = s.execute("for(var i=0;i<33;i++){var a=document.createElement('script');a.src='a.js';document.head.appendChild(a);}").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert!(s.take_script_requests().is_empty());
    let mut s = session();
    s.execute("var a=document.createElement('script');a.src='a.js';document.head.appendChild(a);")
        .unwrap();
    let requests = s.take_script_requests();
    let oversized = " ".repeat(MAX_DYNAMIC_SCRIPT_BYTES + 1);
    assert_eq!(
        s.complete_scripts(&[response(requests[0].id, Some(&oversized))])
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(s.scripts_disabled);
}

#[test]
fn invalid_sources_dispatch_error_and_unsupported_modules_remain_explicit() {
    let mut s = session();
    s.execute("var a=document.createElement('script');a.src='';document.head.appendChild(a);")
        .unwrap();
    assert_eq!(s.take_script_requests()[0].src, "noir-invalid-script:");
    let error = s.execute("var m=document.createElement('script');m.type='module';m.src='a.mjs';document.head.appendChild(m);").unwrap_err();
    assert!(error.message.contains("Dynamic module"));
    let error = s.execute("var inline=document.createElement('script');inline.text='var notRun=true;';document.body.appendChild(inline);").unwrap_err();
    assert!(error.message.contains("Dynamic inline"));
    assert!(!s.scripts_disabled);
}
