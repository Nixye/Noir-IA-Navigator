//! Promise assimilation must not mistake a valid, non-thenable DOM node for
//! an unsupported API, or hide errors and resource limits while probing it.
use super::*;

fn session() -> DocumentSession {
    DocumentSession::new(web_html::parse("<p id=x>safe</p>").unwrap())
}

#[test]
fn absent_then_is_undefined_only_after_valid_node_and_read_checks() {
    let mut s = session();
    let element = s.query_selector("#x").unwrap().unwrap();
    let node_count = s.document.nodes.len();
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    for object in [DOC, NODE + element as u64] {
        let before = host.state.read_work;
        assert_eq!(host.get(object, "then").unwrap(), Value::Undefined);
        assert_eq!(host.state.read_work, before + node_count);
        assert!(!host.has_property(object, "then").unwrap());
    }
    assert_eq!(
        host.get(NODE + node_count as u64, "then")
            .unwrap_err()
            .message,
        "Invalid DOM node handle"
    );
    assert_eq!(
        host.get(DOC, "unimplementedApi").unwrap_err().message,
        "Unsupported document property unimplementedApi"
    );
    assert_eq!(
        host.get(NODE + element as u64, "unimplementedApi")
            .unwrap_err()
            .message,
        "Unsupported DOM property unimplementedApi"
    );
    host.state.read_work = 2_000_000;
    assert_eq!(host.get(DOC, "then").unwrap_err().kind, ErrorKind::Resource);
}

#[test]
fn document_element_and_detached_text_resolve_with_identity_in_microtasks() {
    let mut s = session();
    s.execute(
        r"
        const element=document.getElementById('x');
        const text=document.createTextNode('detached');
        let result='';let rejected='';
        Promise.resolve(document).then(function(value){
            result+='document:'+(value===document)+':'+value.nodeType+';';
        }).catch(function(error){rejected=error.message;});
        new Promise(function(resolve){resolve(element);}).then(function(value){
            result+='element:'+(value===element)+':'+value.nodeType+';';
        }).catch(function(error){rejected=error.message;});
        Promise.resolve(text).then(function(value){
            result+='text:'+(value===text)+':'+value.nodeType+':'+value.isConnected+';';
        }).catch(function(error){rejected=error.message;});
        const synchronous=result;
        ",
    )
    .unwrap();
    assert_eq!(
        s.execute("synchronous+':'+rejected+':'+result;").unwrap(),
        Value::string("::document:true:9;element:true:1;text:true:3:false;")
    );
    assert_eq!(
        s.execute("document.then===undefined && !('then' in document) && element.then===undefined && !('then' in element) && text.then===undefined && !('then' in text);").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn unsupported_dom_reads_still_reject_and_preserve_prior_effects() {
    let mut s = session();
    s.execute(
        r"
        let documentError='';let nodeError='';let getterError='';let probes=0;
        Promise.resolve(document).then(function(value){
            document.getElementById('x').textContent='before rejection';
            return value.unimplementedApi;
        }).catch(function(error){documentError=error.message;});
        async function fail(){
            const node=await document.getElementById('x');
            return node.unimplementedApi;
        }
        fail().catch(function(error){nodeError=error.message;});
        const thenable={};
        Object.defineProperty(thenable,'then',{get:function(){probes++;throw 'getter rejection';}});
        Promise.resolve(thenable).catch(function(error){getterError=error;});
        ",
    )
    .unwrap();
    assert_eq!(
        s.execute("documentError+':'+nodeError+':'+getterError+':'+probes;")
            .unwrap(),
        Value::string(
            "Unsupported document property unimplementedApi:Unsupported DOM property unimplementedApi:getter rejection:1"
        )
    );
    let id = s.query_selector("#x").unwrap().unwrap();
    assert_eq!(s.document.text_content(id), "before rejection");
    assert!(!s.scripts_disabled);
}

#[test]
fn suspended_dom_values_and_local_objects_survive_collection() {
    let mut s = session();
    s.execute(
        r"
        let release;let weak;let failure='';
        const ready=new Promise(function(resolve){release=resolve;});
        async function run(){
            const holder={node:document.createElement('span')};
            holder.node.textContent='held';
            weak=new WeakRef(holder);
            await ready;
            const doc=await document;
            const node=await holder.node;
            doc.getElementById('x').textContent=(weak.deref()===holder)+':'+node.textContent+':'+node.isConnected;
        }
        run().catch(function(error){failure=error.message;});
        ",
    )
    .unwrap();
    s.runtime
        .collect_garbage(
            &[],
            &DomHost {
                document: &mut s.document,
                state: &mut s.state,
            },
        )
        .unwrap();
    s.execute("release(1);").unwrap();
    assert_eq!(s.execute("failure;").unwrap(), Value::string(""));
    let id = s.query_selector("#x").unwrap().unwrap();
    assert_eq!(s.document.text_content(id), "true:held:false");
}

#[test]
fn assimilation_resource_failure_is_fatal_and_rolls_back_vm() {
    for value in ["document", "node"] {
        let mut s = session();
        s.execute("var changed=0;var node=document.getElementById('x');")
            .unwrap();
        s.state.read_work = 2_000_000;
        let error = s
            .execute(&format!(
                "changed=1;Promise.resolve({value}).catch(function(){{changed=2;}});"
            ))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{value}");
        assert_eq!(error.message, "DOM read/traversal work budget exceeded");
        assert_eq!(s.runtime.global_value("changed"), Some(Value::Number(0.)));
        assert_eq!(s.state.read_work, 2_000_000);
        assert!(s.scripts_disabled);
        assert_eq!(s.execute("1;").unwrap_err().kind, ErrorKind::Resource);
    }
}
