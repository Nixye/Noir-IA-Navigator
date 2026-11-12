use super::*;
fn session(html: &str) -> DocumentSession {
    DocumentSession::new(web_html::parse(html).unwrap())
}
fn check(html: &str, source: &str) {
    assert_eq!(
        session(html).execute(source).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}
#[test]
fn snapshot_and_live_share_item_and_foreach_intrinsics() {
    check(
        "<main id=r><i></i></main>",
        "const r=document.getElementById('r');const a=r.childNodes;const b=r.querySelectorAll('i');a.item===b.item&&a.forEach===b.forEach&&a.forEach===Array.prototype.forEach&&a.item.call(b,0)===b[0]&&b.item.call(a,0)===a[0]&&b.item(99)===null&&('item' in b)&&('forEach' in b);",
    );
}
#[test]
fn snapshot_visits_original_members_despite_detach_and_append() {
    check(
        "<main id=r><i id=a></i><i id=b></i><i id=c></i></main>",
        "const r=document.getElementById('r');const n=r.querySelectorAll('i');let seen='';const result=n.forEach(function(v,k,o){seen+=v.id+k+(o===n);v.remove();r.appendChild(document.createElement('b'));});seen==='a0trueb1truec2true'&&n.length===3&&result===undefined;",
    );
}
#[test]
fn live_remove_shifts_members_and_skips_missing_tail() {
    check(
        "<main id=r><i id=a></i><i id=b></i><i id=c></i></main>",
        "const r=document.getElementById('r');let seen='';r.childNodes.forEach(function(v,k){seen+=v.id+k;if(k===0){v.remove();}});seen==='a0c1';",
    );
}
#[test]
fn live_reordering_can_visit_same_node_twice_without_deduplication() {
    check(
        "<main id=r><i id=a></i><i id=b></i><i id=c></i></main>",
        "const r=document.getElementById('r');let seen='';r.childNodes.forEach(function(v,k){seen+=v.id;if(k===0){r.appendChild(v);}});seen==='aca';",
    );
}
#[test]
fn append_beyond_initial_length_is_ignored_but_replacement_inside_is_visited() {
    check(
        "<main id=r><i id=a></i><i id=b></i></main>",
        "const r=document.getElementById('r');let seen='';r.childNodes.forEach(function(v,k){seen+=v.id;if(k===0){r.lastChild.remove();const x=document.createElement('i');x.id='x';r.appendChild(x);const y=document.createElement('i');y.id='y';r.appendChild(y);}});seen==='ax'&&r.childNodes.length===3;",
    );
}
#[test]
fn html_collection_has_no_foreach_but_generic_borrow_works() {
    check(
        "<main id=r><i></i><b></b></main>",
        "const r=document.getElementById('r');const c=r.children;let count=0;let same=true;r.childNodes.forEach.call(c,function(v,k,o){count++;same=same&&o===c&&v===c[k];});c.forEach===undefined&&!('forEach' in c)&&count===2&&same;",
    );
}
#[test]
fn snapshot_item_coercion_is_observable_and_uses_retained_snapshot_members() {
    check(
        "<main id=r><i id=a></i><i id=b></i></main>",
        "const r=document.getElementById('r');const n=r.querySelectorAll('i');let calls=0;const a=n[0];const result=n.item({valueOf:function(){calls++;a.remove();return 4294967296;}});calls===1&&result===a&&n.item(-1)===null&&n.item(NaN)===a&&n.item(1.9)===n[1];",
    );
}
#[test]
fn html_collection_item_cannot_borrow_snapshot_or_live_brands() {
    check(
        "<main id=r><i></i></main>",
        "const r=document.getElementById('r');const c=r.children;const n=r.querySelectorAll('i');let calls=0;let errors=0;const arg={valueOf:function(){calls++;return 0;}};try{c.item.call(n,arg);}catch(e){errors++;}try{n.item.call(c,arg);}catch(e){errors++;}try{n.item.call({},arg);}catch(e){errors++;}errors===3&&calls===0;",
    );
}
#[test]
fn empty_nodelist_validates_callback_and_this_arg_is_uncoerced() {
    check(
        "<main id=r><i></i></main>",
        "let errors=0;try{document.querySelectorAll('missing').forEach(null);}catch(e){errors++;}let correct=true;document.querySelectorAll('i').forEach(function(){'use strict';correct=correct&&this===null;},null);errors===1&&correct;",
    );
}
#[test]
fn callback_throw_preserves_prefix_and_no_mid_loop_microtasks() {
    check(
        "<main id=r><i id=a></i><i id=b></i><i id=c></i></main>",
        "let log='';let caught=false;const marker={};try{document.querySelectorAll('i').forEach(function(v){log+=v.id;Promise.resolve().then(function(){log+='job';});if(v.id==='b'){throw marker;}});}catch(e){caught=e===marker;}caught&&log==='ab';",
    );
}
#[test]
fn retained_lists_survive_gc_and_nested_iteration_without_new_list_slots() {
    let mut s = session("<main id=r><i></i><b></b></main>");
    s.execute("var r=document.getElementById('r');var n=r.childNodes;var snapshot=r.querySelectorAll('*');").unwrap();
    s.runtime
        .collect_garbage(
            &[],
            &DomHost {
                document: &mut s.document,
                state: &mut s.state,
            },
        )
        .unwrap();
    assert_eq!(
        s.execute(
            "let count=0;n.forEach(function(){snapshot.forEach(function(){count++;});});count===4;"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(s.state.lists.len(), 1);
    assert_eq!(s.state.collections.queries.len(), 1);
}
#[test]
fn resource_failure_in_callback_rolls_back_dom_but_preserves_original_collections() {
    let mut s = session("<main id=r><i id=old></i></main>");
    s.execute("var r=document.getElementById('r');var n=r.childNodes;")
        .unwrap();
    assert_eq!(
        s.execute("n.forEach(function(){r.innerHTML='<b></b>';while(true){}});")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(s.scripts_disabled);
    assert_eq!(s.state.collections.queries.len(), 1);
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    let node = host.get(BASE, "0").unwrap().as_host().unwrap();
    assert_eq!(host.get(node, "id").unwrap(), Value::string("old"));
}

#[test]
fn direct_extracted_and_borrowed_routes_agree_for_both_nodelists() {
    check(
        "<main id=r><i></i><b></b></main>",
        "const r=document.getElementById('r');const live=r.childNodes;const snapshot=r.querySelectorAll('*');let calls=0;const cb=function(v,k,o){if(v===o[k]){calls++;}};live.forEach(cb);snapshot.forEach(cb);const f=live.forEach;f.call(live,cb);f.call(snapshot,cb);const g=snapshot.forEach;g.call(live,cb);g.call(snapshot,cb);const item=snapshot.item;calls===12&&live.item(0)===snapshot.item(0)&&item.call(live,1)===live[1]&&item.call(snapshot,1)===snapshot[1];",
    );
}
