use super::*;

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
