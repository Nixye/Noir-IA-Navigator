use super::*;

#[test]
fn iterative_evaluation_fixture_preserves_dom_effects_on_one_mib_stack() {
    std::thread::Builder::new()
        .name("iterative-evaluation-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let html = include_str!("../../../fixtures/javascript-iterative-evaluation.html");
            let source = html
                .split_once("<script>")
                .unwrap()
                .1
                .split_once("</script>")
                .unwrap()
                .0;
            let mut session = DocumentSession::new(web_html::parse(html).unwrap());
            session.execute(source).unwrap();
            assert!(
                session
                    .document
                    .text_content(0)
                    .contains("PASS: expressoes iterativas, referencias e bridges.")
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn iterative_evaluation_resource_preserves_dom_transaction_and_skips_cleanup() {
    let mut session = DocumentSession::new(web_html::parse("<p id=out>stable</p>").unwrap());
    session.execute("var stable=7;").unwrap();
    let error = session.execute("stable=8;document.getElementById('out').textContent='bad';queueMicrotask(function(){document.getElementById('out').textContent='late';});function recurse(n){return n?1+(2+(3+recurse(n-1))):0;}try{recurse(1000);}finally{document.getElementById('out').textContent='cleanup';}").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(session.scripts_disabled);
    assert_eq!(
        session.runtime.global_value("stable"),
        Some(Value::Number(7.))
    );
    let text = session.document.text_content(0);
    assert!(text.contains("stable"));
    for forbidden in ["bad", "late", "cleanup"] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn error_ancestor_fixture_updates_dom_on_one_mib_stack() {
    std::thread::Builder::new()
        .name("error-ancestors-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let html = include_str!("../../../fixtures/javascript-error-ancestors.html");
            let source = html
                .split_once("<script>")
                .unwrap()
                .1
                .split_once("</script>")
                .unwrap()
                .0;
            let mut session = DocumentSession::new(web_html::parse(html).unwrap());
            session.execute(source).unwrap();
            assert!(
                session
                    .document
                    .text_content(0)
                    .contains("PASS: heranca Error, receivers e limites.")
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn error_ancestors_survive_dynamic_scripts_microtasks_and_retained_execution() {
    let mut session = DocumentSession::new(web_html::parse("<p id=out>waiting</p>").unwrap());
    session.execute("var trace='';function callback(){trace+=Object.getPrototypeOf(callback).name;document.getElementById('out').textContent=trace;}Object.setPrototypeOf(callback,Error);const script=document.createElement('script');script.text=\"Object.setPrototypeOf(callback,window.TypeError);queueMicrotask(callback);\";document.head.appendChild(script);").unwrap();
    assert_eq!(
        session
            .execute("Object.getPrototypeOf(callback)===TypeError&&trace==='TypeError';")
            .unwrap(),
        Value::Bool(true)
    );
    session
        .execute("Object.setPrototypeOf(callback,window.Error);queueMicrotask(callback);")
        .unwrap();
    assert!(session.document.text_content(0).contains("TypeErrorError"));
    assert_eq!(
        session
            .execute("Object.getPrototypeOf(callback)===Error&&callback instanceof Function;")
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn function_prototype_fixture_changes_real_inheritance_and_dom() {
    let html = include_str!("../../../fixtures/javascript-function-prototypes.html");
    let source = html
        .split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let mut s = DocumentSession::new(web_html::parse(html).unwrap());
    s.execute(source).unwrap();
    assert!(
        s.document
            .text_content(0)
            .contains("Protótipos e receptores de funções ativos.")
    );
}

#[test]
fn function_prototypes_survive_retained_scripts_and_host_callback_receivers() {
    let mut s = DocumentSession::new(web_html::parse("<p id=out>waiting</p>").unwrap());
    s.execute("var observed='';function callback(){observed+=callback.marker;document.getElementById('out').textContent=observed;}Object.setPrototypeOf(callback,{marker:'x'});queueMicrotask(callback);").unwrap();
    assert_eq!(s.execute("Object.getPrototypeOf(callback).marker==='x'&&callback.call===undefined&&observed==='x';").unwrap(),Value::Bool(true));
    s.execute("Object.setPrototypeOf(callback,{marker:'y'});queueMicrotask(callback);")
        .unwrap();
    assert!(s.document.text_content(0).contains("xy"));
}

#[test]
fn promise_inspection_fixture_reaches_existing_finally_and_updates_dom() {
    let html = include_str!("../../../fixtures/javascript-promise-inspection.html");
    let source = html
        .split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let mut s = DocumentSession::new(web_html::parse(html).unwrap());
    s.execute(source).unwrap();
    assert!(
        s.document
            .text_content(0)
            .contains("Inspeção e microtasks Promise ativos.")
    );
    assert_eq!(s.execute("('prototype' in window.Promise)&&window.Promise===Promise&&typeof Promise.prototype.finally==='function';").unwrap(),Value::Bool(true));
}

#[test]
fn oversized_promise_inspection_rolls_back_dom_globals_and_pending_jobs() {
    let mut s = DocumentSession::new(web_html::parse("<p id=out>safe</p>").unwrap());
    s.execute("var stable=1;").unwrap();
    let error=s.execute("stable=2;document.getElementById('out').textContent='bad';queueMicrotask(function(){document.getElementById('out').textContent='late';});'x'.repeat(1025) in Promise;").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert_eq!(s.runtime.global_value("stable"), Some(Value::Number(1.)));
    assert!(s.document.text_content(0).contains("safe"));
    assert!(!s.document.text_content(0).contains("late"));
}

#[test]
fn number_descriptor_fixture_updates_dom_and_preserves_constant_flags() {
    let html = include_str!("../../../fixtures/javascript-number-descriptors.html");
    let source = html
        .split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let mut s = DocumentSession::new(web_html::parse(html).unwrap());
    s.execute(source).unwrap();
    assert!(
        s.document
            .text_content(0)
            .contains("Constantes e descritores Number ativos.")
    );
    assert_eq!(s.execute("Number===window.Number&&globalThis.Number===Number&&Object.getOwnPropertyDescriptor(Number,'MAX_SAFE_INTEGER').configurable===false;").unwrap(),Value::Bool(true));
}

#[test]
fn number_descriptors_share_reentrant_script_state_and_accessor_receivers() {
    let mut s = DocumentSession::new(web_html::parse("<p></p>").unwrap());
    s.execute("var trace='';Object.defineProperty(Number,'demo',{get:function(){return this===Number?7:0;},configurable:true});const script=document.createElement('script');script.text=\"trace+=String(Number.demo);Number.shared=17;queueMicrotask(function(){trace+='q'+String(Number.shared);});\";document.head.appendChild(script);trace+='p';").unwrap();
    assert_eq!(
        s.execute("trace==='7pq17'&&Number.shared===17;").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn window_map_polyfill_property_and_native_constructor_are_independent() {
    let mut s = DocumentSession::new(web_html::parse("<p id=out>waiting</p>").unwrap());
    assert_eq!(s.execute("const original=Map;function Polyfill(){this.tag=23;}Object.defineProperty(window,'Map',{configurable:true,writable:true,value:Polyfill});const m=new Map();document.getElementById('out').textContent=String(m.tag);const d=Object.getOwnPropertyDescriptor(window,'Map');Map===Polyfill&&window.Map===Map&&globalThis.Map===Map&&original!==Map&&d.configurable&&d.writable&&!d.enumerable;").unwrap(),Value::Bool(true));
    assert!(s.document.text_content(0).contains("23"));
}
#[test]
fn accessor_receiver_global_aliases_and_host_permissions_stay_distinct() {
    let mut s = DocumentSession::new(web_html::parse("<p id=out>waiting</p>").unwrap());
    assert_eq!(s.execute("let count=0;Object.defineProperty(window,'ready',{get:function(){count++;return this===window&&document.body!==null;},configurable:true});const d=Object.getOwnPropertyDescriptor(window,'ready');const untouched=count===0;let rejected=0;try{Object.defineProperty(document,'x',{value:3});}catch(e){rejected++;}try{Object.defineProperty(window,'location',{value:'https://example.test/'});}catch(e){rejected++;}untouched&&ready&&self.ready&&count===2&&rejected===2&&typeof d.get==='function';").unwrap(),Value::Bool(true));
}
#[test]
fn global_descriptors_work_during_inline_script_reentrancy_and_microtasks() {
    let mut s = DocumentSession::new(web_html::parse("<p></p>").unwrap());
    s.execute("var trace='';var reads=0;Object.defineProperty(window,'x',{get:function(){reads++;return this===window?7:0;},configurable:true});const script=document.createElement('script');script.text=\"trace+=String(x);queueMicrotask(function(){trace+='q'+String(window.x);});\";document.head.appendChild(script);trace+='p';").unwrap();
    assert_eq!(
        s.execute("trace==='7pq7'&&reads===2;").unwrap(),
        Value::Bool(true)
    );
}
#[test]
fn fatal_resource_rollback_restores_global_metadata_and_bindings() {
    let mut s = DocumentSession::new(web_html::parse("<p id=out>safe</p>").unwrap());
    s.execute("var kept=1;Object.defineProperty(window,'Map',{value:17,writable:true,configurable:true});").unwrap();
    let error=s.execute("kept=2;Object.defineProperty(window,'Map',{value:99,writable:false});document.getElementById('out').textContent='bad';while(true){}").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert_eq!(s.runtime.global_value("kept"), Some(Value::Number(1.)));
    assert_eq!(s.runtime.global_value("Map"), Some(Value::Number(17.)));
    assert!(s.document.text_content(0).contains("safe"));
    assert!(s.take_script_requests().is_empty());
}

#[test]
fn bounded_stack_fixture_runs_with_dom_on_one_mib_stack() {
    std::thread::Builder::new()
        .name("bindings-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let html = include_str!("../../../fixtures/javascript-stack-budgets.html");
            let source = html
                .split_once("<script>")
                .unwrap()
                .1
                .split_once("</script>")
                .unwrap()
                .0;
            let mut session = DocumentSession::new(web_html::parse(html).unwrap());
            session.execute(source).unwrap();
            assert!(
                session
                    .document
                    .text_content(0)
                    .contains("Expressões, blocos e chamadas executados com limites ativos.")
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn cumulative_statement_depth_is_fatal_and_rolls_back_dom_on_one_mib_stack() {
    std::thread::Builder::new()
        .name("bindings-depth-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut session = DocumentSession::new(web_html::parse("<p id=out>stable</p>").unwrap());
            session.execute("var stable=7;").unwrap();
            let nested = format!("{}f();{}", "{".repeat(10), "}".repeat(10));
            let source = format!("stable=8;document.getElementById('out').textContent='bad';queueMicrotask(function(){{document.getElementById('out').textContent='late'}});function f(){{{nested}}}f();");
            let error = session.execute(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Statement evaluation depth exceeded");
            assert!(session.scripts_disabled);
            assert_eq!(session.runtime.global_value("stable"), Some(Value::Number(7.)));
            assert!(session.document.text_content(0).contains("stable"));
            assert!(!session.document.text_content(0).contains("bad"));
            assert!(!session.document.text_content(0).contains("late"));
        })
        .unwrap()
        .join()
        .unwrap();
}
