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
fn repeated_reads_have_stable_distinct_identity_without_snapshot_allocation() {
    let mut s = session("<main id=r>text<p></p></main>");
    assert_eq!(s.execute("const r=document.getElementById('r');const a=r.children;const b=r.childNodes;let same=true;for(let i=0;i<100;i++){same=same&&a===r.children&&b===r.childNodes;}same&&a!==b&&a.length===1&&b.length===2;").unwrap(), Value::Bool(true));
    assert_eq!(s.state.collections.queries.len(), 2);
    assert!(s.state.lists.is_empty());
}

#[test]
fn direct_collections_track_insert_remove_and_order_not_descendants() {
    check(
        "<main id=r>text<p id=a><b></b></p><p id=b></p></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const n=r.childNodes;
        const a=c[0];const b=c[1];const x=document.createElement('i');r.insertBefore(x,a);
        const inserted=c.length===3&&c[0]===x&&n[1]===x;
        a.remove();r.appendChild(x);
        inserted&&c.length===2&&c[0]===b&&c[1]===x&&n.length===3&&c.item(2)===null&&n[3]===undefined;
    "#,
    );
}

#[test]
fn reparenting_updates_both_held_collections() {
    check(
        "<div id=a><i id=x></i></div><div id=b></div>",
        r#"
        const a=document.getElementById('a');const b=document.getElementById('b');
        const ac=a.children;const bc=b.children;const x=ac[0];b.appendChild(x);
        ac.length===0&&bc.length===1&&bc[0]===x&&x.parentNode===b&&ac===a.children;
    "#,
    );
}

#[test]
fn inner_html_and_text_replacement_keep_handles_but_not_old_membership() {
    check(
        "<main id=r><p id=old></p></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const n=r.childNodes;const old=c[0];
        r.innerHTML='hello<i id=new></i>';
        const replaced=c.length===1&&c[0].id==='new'&&n.length===2&&old.parentNode===null;
        r.textContent='only';replaced&&c.length===0&&n.length===1&&n[0].nodeType===3&&n===r.childNodes;
    "#,
    );
}

#[test]
fn document_and_text_have_appropriate_subtypes() {
    check(
        "<p id=r>hello</p>",
        r#"
        const c=document.children;const n=document.childNodes;const t=document.getElementById('r').firstChild;
        const empty=t.childNodes;
        c===document.children&&n===document.childNodes&&c!==n&&c[0]===document.documentElement
            &&n[0]===document.documentElement&&empty===t.childNodes&&empty.length===0
            &&empty.item(0)===null&&empty[0]===undefined&&!('children' in t)&&t.children===undefined;
    "#,
    );
}

#[test]
fn detached_root_and_detached_descendant_remain_live() {
    check(
        "<main id=r><p id=p></p></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const p=c[0];const pc=p.childNodes;
        r.remove();p.appendChild(document.createTextNode('x'));r.appendChild(document.createElement('b'));
        c.length===2&&pc.length===1&&pc[0].textContent==='x'&&!r.isConnected&&c===r.children;
    "#,
    );
}

#[test]
fn inert_roots_use_actual_direct_children_without_changing_tag_policy() {
    let mut s =
        session("<template id=t><p>hidden</p></template><noscript id=n><b>hidden</b></noscript>");
    for tag in ["template", "noscript"] {
        let id = s
            .document
            .elements()
            .find_map(|(id, e)| (e.tag == tag).then_some(id))
            .unwrap();
        let expected = s.document.nodes[id].children.len();
        let elements = s.document.nodes[id]
            .children
            .iter()
            .filter(|&&id| s.document.element(id).is_some())
            .count();
        let mut host = DomHost {
            document: &mut s.document,
            state: &mut s.state,
        };
        let nodes = host.child_collection(id, false).unwrap().as_host().unwrap();
        let children = host.child_collection(id, true).unwrap().as_host().unwrap();
        assert_eq!(
            host.get(nodes, "length").unwrap(),
            Value::Number(expected as f64)
        );
        assert_eq!(
            host.get(children, "length").unwrap(),
            Value::Number(elements as f64)
        );
    }
    assert_eq!(s.execute("document.getElementsByTagName('p').length===0&&document.getElementsByTagName('b').length===0;").unwrap(), Value::Bool(true));
}

#[test]
fn html_names_use_first_child_in_order_and_do_not_leak_to_node_list() {
    check(
        "<main id=r><input name=shared><i id=shared></i><b id=9></b><em id=00></em></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const n=r.childNodes;
        c.shared===c[0]&&c.namedItem('shared')===c[0]&&c.namedItem('9')===c[2]
            &&c[9]===undefined&&!('9' in c)&&c['00']===c[3]&&('shared' in c)
            &&n.shared===undefined&&!('shared' in n)&&n.namedItem===undefined&&!('namedItem' in n)
            &&n['00']===undefined&&!('00' in n);
    "#,
    );
}

#[test]
fn item_uses_uint32_and_current_membership_after_reentrant_coercion() {
    check(
        "<main id=r><i id=a></i><b id=b></b></main>",
        r#"
        const r=document.getElementById('r');const n=r.childNodes;let calls=0;const old=n[0];
        const result=n.item({valueOf:function(){calls++;old.remove();return 0;}});
        calls===1&&result.id==='b'&&n.item(4294967296)===result&&n.item(NaN)===result
            &&n.item(-1)===null&&n.item(0.9)===result&&n.item(Infinity)===result;
    "#,
    );
}

#[test]
fn named_item_coercion_observes_post_conversion_dom() {
    check(
        "<main id=r><i id=a></i><b id=b></b></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;let calls=0;
        const result=c.namedItem({toString:function(){calls++;c[0].id='b';return 'b';}});
        calls===1&&result===c[0]&&c.namedItem('')===null;
    "#,
    );
}

#[test]
fn borrowed_methods_validate_subtype_before_argument_coercion() {
    check(
        "<main id=r><i></i></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const n=r.childNodes;let calls=0;let errors=0;
        const arg={valueOf:function(){calls++;return 0;},toString:function(){calls++;return 'x';}};
        try{c.item.call(n,arg);}catch(e){errors++;}
        try{n.item.call(c,arg);}catch(e){errors++;}
        try{c.namedItem.call(n,arg);}catch(e){errors++;}
        try{n.item.call({},arg);}catch(e){errors++;}
        try{n.item();}catch(e){errors++;}
        // R4 intentionally unifies item across live and static NodeLists.
        const snapshot=r.querySelectorAll('i');
        const rejectedBeforeCoercion=errors===5&&calls===0;
        const borrowed=n.item.call(snapshot,arg);
        rejectedBeforeCoercion&&calls===1&&borrowed===snapshot[0]&&snapshot.item===n.item&&('item' in snapshot)
            &&snapshot.item.call(n,0)===n[0]&&n.item.call(n,0)===c.item.call(c,0)&&n.item.name==='item'&&n.item.length===1;
    "#,
    );
}

#[test]
fn live_descriptor_limit_is_shared_and_existing_handles_work_at_capacity() {
    let mut s = session("<p></p>");
    assert_eq!(s.execute("const c=document.children;const n=document.childNodes;for(let i=0;i<62;i++){document.getElementsByTagName('x'+i);}let same=true;for(let i=0;i<65;i++){same=same&&document.children===c&&document.childNodes===n;}same;").unwrap(), Value::Bool(true));
    assert_eq!(s.state.collections.queries.len(), 64);
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert_eq!(host.child_collection(0, true).unwrap(), Value::Host(BASE));
    assert_eq!(
        host.child_collection(0, false).unwrap(),
        Value::Host(BASE + 1)
    );
    assert_eq!(
        host.child_collection(1, false).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(host.state.collections.queries.len(), 64);
}

#[test]
fn snapshot_lists_remain_distinct_static_and_have_canonical_indices() {
    let mut s = session("<main id=r><p id=p></p></main>");
    assert_eq!(s.execute("const r=document.getElementById('r');const a=r.querySelectorAll('p');const b=r.querySelectorAll('p');const live=r.children;a[0].remove();a!==b&&a.length===1&&b.length===1&&live.length===0&&a['00']===undefined&&!('00' in a)&&a[0]===b[0];").unwrap(),Value::Bool(true));
    assert_eq!(s.state.lists.len(), 2);
}

#[test]
fn snapshot_quota_is_unchanged_and_independent_of_live_reads() {
    let mut s = session("");
    s.execute("for(let i=0;i<100;i++){document.children;}for(let i=0;i<64;i++){document.querySelectorAll('missing');}").unwrap();
    assert_eq!(s.state.lists.len(), 64);
    assert_eq!(s.state.collections.queries.len(), 1);
    assert_eq!(
        s.execute("document.querySelectorAll('missing');")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(s.state.lists.len(), 64);
}

#[test]
fn direct_reads_and_descriptor_lookup_charge_shared_work_before_allocation() {
    let mut s = session("<p></p>");
    s.execute("var children=document.children;").unwrap();
    s.state.read_work = 2_000_000;
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert_eq!(
        host.get(BASE, "length").unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(
        host.child_collection(0, false).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(host.state.collections.queries.len(), 1);
}

#[test]
fn lookup_name_limit_and_invalid_handle_fail_without_dom_mutation() {
    let mut s = session("<p></p>");
    s.execute("var c=document.children;").unwrap();
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert_eq!(
        host.get(BASE, &"x".repeat(1025)).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert!(host.get(BASE + 63, "length").is_err());
    assert_eq!(host.state.collections.queries.len(), 1);
}

#[test]
fn readonly_collections_reject_writes_and_retained_handles_survive_gc() {
    let mut s = session("<main id=r><i></i></main>");
    s.execute("var r=document.getElementById('r');var c=r.children;var n=r.childNodes;")
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
    assert_eq!(s.execute("let errors=0;try{n.length=0;}catch(e){errors++;}try{c[0]=null;}catch(e){errors++;}try{n.extra=1;}catch(e){errors++;}r.appendChild(document.createElement('b'));errors===3&&c.length===2&&n.length===2&&n===r.childNodes;").unwrap(),Value::Bool(true));
}

#[test]
fn fatal_execution_rolls_back_new_descriptors_and_membership() {
    let mut s = session("<main id=r><i id=old></i></main>");
    s.execute("var r=document.getElementById('r');var c=r.children;")
        .unwrap();
    assert_eq!(
        s.execute("r.innerHTML='<b></b>';r.childNodes;while(true){}")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(s.state.collections.queries.len(), 1);
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    let value = host.get(BASE, "0").unwrap().as_host().unwrap();
    assert_eq!(host.get(value, "id").unwrap(), Value::string("old"));
}

#[test]
fn snapshot_member_limit_is_not_relaxed_by_live_collection_support() {
    let mut s = session("<p></p>");
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert!(host.list(vec![1; 4096]).is_ok());
    assert_eq!(
        host.list(vec![1; 4097]).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(host.state.lists.len(), 1);
}

#[test]
fn direct_descriptor_does_not_alias_descendant_tag_query() {
    check(
        "<main id=r><i><b></b></i></main>",
        r#"
        const r=document.getElementById('r');const c=r.children;const tags=r.getElementsByTagName('*');
        c!==tags&&c.length===1&&tags.length===2&&c[0]===tags[0]&&c.item===tags.item;
    "#,
    );
}

#[test]
fn throwing_coercion_does_not_return_stale_member_or_allocate_descriptor() {
    let mut s = session("<main id=r><i></i></main>");
    assert_eq!(s.execute("const r=document.getElementById('r');const n=r.childNodes;let calls=0;let errors=0;try{n.item({valueOf:function(){calls++;throw 'stop';}});}catch(e){errors++;}calls===1&&errors===1&&n.length===1;").unwrap(),Value::Bool(true));
    assert_eq!(s.state.collections.queries.len(), 1);
}
