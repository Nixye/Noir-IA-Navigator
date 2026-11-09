use super::tests::{check, response, session};
use super::*;

#[test]
fn inline_append_is_synchronous_global_and_does_not_emit_load() {
    let mut s = session();
    s.execute(r#"
        var order='before';var loads=0;var node=document.createElement('script');
        node.text="order+='|inline';var addedGlobal=42;var scriptMatches=document.currentScript===node;";
        node.onload=function(){loads++;};
        function insert(){var hiddenLocal=7;document.head.appendChild(node);order+='|after';}
        insert();
    "#).unwrap();
    check(
        &mut s,
        "order==='before|inline|after' && addedGlobal===42 && window.addedGlobal===42 && scriptMatches && loads===0 && document.currentScript===null;",
    );
    assert!(s.take_script_requests().is_empty());
}

#[test]
fn inline_nested_script_restores_current_script_and_defers_jobs() {
    let mut s = session();
    s.execute(r#"
        var order='';var outer=document.createElement('script');
        outer.text="order+='A';var inner=document.createElement('script');inner.text=\"order+='B';queueMicrotask(function(){order+='Q';var microCurrent=document.currentScript;});\";document.body.appendChild(inner);order+=(document.currentScript===outer?'C':'BAD');";
        document.head.appendChild(outer);order+='D';
        queueMicrotask(function(){order+=document.currentScript===null?'N':'BAD';});
    "#).unwrap();
    check(&mut s, "order==='ABCDQN' && document.currentScript===null;");
}

#[test]
fn initial_and_external_current_script_clear_before_microtasks_and_callbacks() {
    let html = r#"<script id=initial>
        var seen=document.currentScript.id;
        queueMicrotask(function(){seen+=document.currentScript===null?'|job':'|BAD';});
        var remote=document.createElement('script');remote.src='next.js';remote.id='remote';
        remote.onload=function(){seen+=document.currentScript===null?'|load':'|BAD';};
        document.head.appendChild(remote);
        </script>"#;
    let mut s = DocumentSession::new(web_html::parse(html).unwrap());
    s.set_document_url("https://example.test/index.html")
        .unwrap();
    let report = s.run_initial(&[]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let requests = s.take_script_requests();
    s.complete_scripts(&[response(requests[0].id, Some("seen+='|'+document.currentScript.id;queueMicrotask(function(){seen+=document.currentScript===null?'|job':'|BAD';});"))]).unwrap();
    check(&mut s, "seen==='initial|job|remote|job|load';");
}

#[test]
fn connected_text_setters_and_text_child_insertions_execute_once() {
    for operation in [
        "script.text=source;",
        "script.textContent=source;",
        "script.innerHTML=source;",
        "script.appendChild(document.createTextNode(source));",
        "var text=document.createTextNode('');script.appendChild(text);text.textContent=source;",
    ] {
        let mut s = session();
        s.execute(&format!(r#"var count=0;var source='count++;';var script=document.createElement('script');document.body.appendChild(script);{operation}var immediate=count;script.text='count+=100;';script.remove();document.head.appendChild(script);"#)).unwrap();
        check(&mut s, "count===1 && immediate===1;");
        assert!(s.take_script_requests().is_empty());
    }
}

#[test]
fn inline_insertion_alias_replace_and_nested_batches_keep_order() {
    let mut s = session();
    s.execute(r#"
        var order='';var holder=document.createElement('div');
        var a=document.createElement('script');a.text="order+='a';var inner=document.createElement('script');inner.text=\"order+='i';\";document.head.appendChild(inner);order+='A';";
        var b=document.createElement('script');b.text="order+='b';";
        holder.appendChild(a);holder.appendChild(b);
        var append=document.body.appendChild;append.call(document.body,holder);order+='B';
        var c=document.createElement('script');c.text="order+='c';";
        document.body.replaceChild(c,holder);
    "#).unwrap();
    check(&mut s, "order==='aiAbBc';");
}

#[test]
fn inline_syntax_and_runtime_errors_report_without_throwing_to_insert_caller() {
    for source in ["effect=1;throw 'ordinary';", "effect=1;let = ;"] {
        let mut s = session();
        // Source is inserted through direct host data so malformed JS quoting
        // cannot turn this into a test of the outer script's parser.
        s.execute("var effect=0;var caught=0;var errors=0;var after=0;window.onerror=function(){errors++;};var script=document.createElement('script');").unwrap();
        let node = s.runtime.global_value("script").unwrap().as_host().unwrap();
        DomHost {
            document: &mut s.document,
            state: &mut s.state,
        }
        .set(node, "text", Value::string(source))
        .unwrap();
        s.execute("try{document.head.appendChild(script);}catch(e){caught++;}after=1;")
            .unwrap();
        check(
            &mut s,
            "caught===0 && errors===1 && after===1 && document.currentScript===null;",
        );
        assert_eq!(
            s.runtime.global_value("effect"),
            Some(Value::Number(if source.contains("throw") {
                1.
            } else {
                0.
            }))
        );
        assert_eq!(s.console_messages().len(), 1);
        assert!(!s.scripts_disabled);
    }
}

#[test]
fn setter_error_handler_jobs_wait_until_outer_script_finishes() {
    let mut s = session();
    s.execute(
        r#"
        var order='';window.onerror=function(){order+='E';queueMicrotask(function(){order+='Q';});};
        var script=document.createElement('script');document.body.appendChild(script);
        script.text="order+='I';throw 'ordinary';";order+='A';
    "#,
    )
    .unwrap();
    assert_eq!(s.execute("order;").unwrap(), Value::string("IEAQ"));
}

#[test]
fn inline_clones_and_inner_html_remain_inert_after_start() {
    let mut s = session();
    s.execute(r#"
        var count=0;var a=document.createElement('script');a.text='count++;';
        var fresh=a.cloneNode(true);document.head.appendChild(a);document.head.appendChild(fresh);
        var spent=a.cloneNode(true);document.head.appendChild(spent);
        var holder=document.createElement('div');holder.innerHTML='<script>count+=100;</script>';
        document.body.appendChild(holder);var parsed=holder.firstChild;document.head.appendChild(parsed);
    "#).unwrap();
    check(&mut s, "count===2;");
}

#[test]
fn inline_resource_failure_rolls_back_outer_operation_and_disables_future_work() {
    let mut s = session();
    let error = s
        .execute(
            r#"
        document.getElementById('out').textContent='bad';
        var script=document.createElement('script');script.text='while(true){}';
        try{document.head.appendChild(script);}catch(e){document.title='caught';}
    "#,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert!(s.take_script_requests().is_empty());
    assert!(s.state.dynamic_scripts.inline.is_empty());
    let out = s
        .document
        .elements()
        .find(|(_, e)| e.attr("id") == Some("out"))
        .unwrap()
        .0;
    assert_eq!(s.document.text_content(out), "pending");
}

#[test]
fn recursive_inline_insertion_fails_bounded_without_becoming_catchable() {
    let mut s = session();
    let error = s.execute(r#"
        var source="var next=document.createElement('script');next.text=source;document.head.appendChild(next);";
        var first=document.createElement('script');first.text=source;
        try{document.head.appendChild(first);}catch(e){document.title='caught';}
    "#).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert!(s.state.dynamic_scripts.inline.is_empty());
}

#[test]
fn inline_and_external_attempts_share_dynamic_count_budget() {
    let mut s = session();
    let error = s.execute(r#"
        for(var i=0;i<32;i++){var script=document.createElement('script');script.text='var x=1;';document.head.appendChild(script);}
        var external=document.createElement('script');external.src='last.js';document.head.appendChild(external);
    "#).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert!(s.take_script_requests().is_empty());
}
