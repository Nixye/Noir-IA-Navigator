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
fn tag_queries_are_live_ordered_and_share_node_identity() {
    check(
        "<main id=root><p id=a></p><p id=b></p></main>",
        r#"
        const nodes=document.getElementsByTagName('P');
        const root=document.getElementById('root');const a=nodes[0];const b=nodes[1];
        const p=document.createElement('p');p.id='new';root.insertBefore(p,a);
        const first=nodes.length===3&&nodes[0]===p&&nodes.item(1)===a;
        a.remove();root.appendChild(b);
        first&&nodes.length===2&&nodes[1]===b&&nodes[2]===undefined&&nodes.item(2)===null
            &&('0' in nodes)&&!('2' in nodes)&&nodes===document.getElementsByTagName('p');
    "#,
    );
}

#[test]
fn document_and_element_roots_wildcards_empty_queries_and_detached_subtrees() {
    check(
        "<div id=root><span id=a></span>text<div id=b></div></div><span id=outside></span>",
        r#"
        const root=document.getElementById('root');const all=root.getElementsByTagName('*');
        const docs=document.getElementsByTagName('span');root.remove();
        all.length===2&&all[0].id==='a'&&all[1].id==='b'&&docs.length===1
            &&root.getElementsByTagName('div').length===1&&root.getElementsByTagName('').length===0
            &&document.getElementsByTagName('html')[0]===document.documentElement
            &&document.getElementsByTagName('head')[0]===document.head;
    "#,
    );
}

#[test]
fn named_lookup_is_live_and_builtins_win_over_names() {
    check(
        "<p id=a name=shared></p><p id=shared></p><p id=item></p><p id=length></p>",
        r#"
        const nodes=document.getElementsByTagName('p');const a=nodes[0];const b=nodes[1];
        const before=nodes.namedItem('shared')===a&&nodes.shared===a&&nodes.namedItem('')===null
            &&nodes.namedItem('item')===nodes[2]&&typeof nodes.item==='function'&&nodes.length===4;
        a.removeAttribute('name');b.id='renamed';
        before&&nodes.shared===undefined&&!('shared' in nodes)&&nodes.renamed===b
            &&nodes.namedItem('missing')===null&&nodes.missing===undefined;
    "#,
    );
}

#[test]
fn methods_support_detachment_arity_observable_coercion_and_unsigned_indices() {
    check(
        "<p id=one></p><p id=two></p>",
        r#"
        const query=document.getElementsByTagName;const nodes=query.call(document,{toString:function(){return 'P';}});
        const item=nodes.item;const named=nodes.namedItem;let log='';
        const index={valueOf:function(){log+='i';return 1;}};const name={toString:function(){log+='n';return 'two';}};
        query.length===1&&query.name==='getElementsByTagName'&&item.length===1&&named.length===1
            &&item.call(nodes,index)===nodes[1]&&named.call(nodes,name)===nodes[1]&&log==='in'
            &&nodes.item(undefined)===nodes[0]&&nodes.item(-1)===null&&nodes.item(4294967296)===nodes[0]
            &&nodes.item(1.9)===nodes[1]&&nodes.item(NaN)===nodes[0];
    "#,
    );
}

#[test]
fn coercion_happens_before_a_live_lookup_and_exceptions_keep_prior_effects() {
    check(
        "<div id=root><p id=a></p></div>",
        r#"
        const root=document.getElementById('root');const nodes=root.getElementsByTagName('p');
        const arg={valueOf:function(){const p=document.createElement('p');p.id='new';root.appendChild(p);return 1;}};
        const ok=nodes.item(arg).id==='new';let threw=false;
        try{nodes.namedItem({toString:function(){root.firstChild.remove();throw 17;}});}catch(e){threw=e===17;}
        ok&&threw&&nodes.length===1&&nodes[0].id==='new';
    "#,
    );
}

#[test]
fn no_argument_invalid_receiver_and_symbol_conversions_fail_without_query_allocation() {
    for source in [
        "document.getElementsByTagName();",
        "document.getElementsByTagName.call({},'p');",
        "document.getElementsByTagName(Symbol());",
        "document.createTextNode('x').getElementsByTagName('p');",
    ] {
        let mut s = session("");
        assert!(s.execute(source).is_err(), "{source}");
        assert!(s.state.collections.queries.is_empty());
    }
    check(
        "<p></p>",
        r#"
        const nodes=document.getElementsByTagName('p');let n=0;
        try{nodes.item();}catch(e){n++;}try{nodes.item.call(document,0);}catch(e){n++;}
        try{nodes.item(1n);}catch(e){n++;}try{nodes.namedItem(Symbol());}catch(e){n++;}
        n===4&&nodes.length===1;
    "#,
    );
}

#[test]
fn array_from_reads_a_live_collection_without_changing_query_selector_snapshots() {
    check(
        "<p id=a></p><p id=b></p>",
        r#"
        const live=document.getElementsByTagName('p');const snapshot=document.querySelectorAll('p');
        const values=Array.from(live,function(node,index){if(index===0){live[1].remove();}return node===undefined?'gone':node.id;});
        values.join(',')==='a,gone'&&live.length===1&&snapshot.length===2&&snapshot[1].id==='b';
    "#,
    );
}

#[test]
fn inert_boundaries_are_visible_but_descendants_remain_excluded() {
    check(
        "<template id=t><p id=hidden></p></template><noscript><p>hidden</p></noscript><p id=visible></p>",
        r#"
        document.getElementsByTagName('p').length===1&&document.getElementsByTagName('p')[0].id==='visible'
            &&document.getElementsByTagName('template').length===1
            &&document.getElementById('t').getElementsByTagName('*').length===0;
    "#,
    );
}

#[test]
fn distinct_queries_and_names_obey_limits_and_shared_queries_do_not_leak() {
    let mut s = session("");
    assert_eq!(s.execute("for(let i=0;i<100;i++){document.getElementsByTagName('p');}document.getElementsByTagName('p').length;").unwrap(),Value::Number(0.0));
    assert_eq!(s.state.collections.queries.len(), 1);
    for source in [
        "for(let i=0;i<65;i++){document.getElementsByTagName('x'+i);}",
        "document.getElementsByTagName('x'.repeat(1025));",
        "document.getElementsByTagName('p').namedItem('x'.repeat(1025));",
        "document.getElementsByTagName('p')['x'.repeat(1025)];",
    ] {
        let mut s = session("");
        assert_eq!(s.execute(source).unwrap_err().kind, ErrorKind::Resource);
        assert!(s.scripts_disabled);
        assert!(s.state.collections.queries.is_empty());
    }
}

#[test]
fn traversal_budget_is_cumulative_and_invalid_handles_cannot_alias_nodes() {
    let mut s = session("<p id=a></p>");
    s.execute("var nodes=document.getElementsByTagName('p');")
        .unwrap();
    s.state.read_work = 2_000_000;
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert_eq!(
        host.get(BASE, "length").unwrap_err().kind,
        ErrorKind::Resource
    );
    assert!(host.get(BASE + 63, "length").is_err());
}

#[test]
fn element_methods_cannot_be_borrowed_for_document_or_text_receivers() {
    check(
        "<p></p>",
        r#"
        const query=document.body.getElementsByTagName;let calls=0;let failures=0;
        const arg={toString:function(){calls++;return 'p';}};
        try{query.call(document,arg);}catch(e){failures++;}
        try{query.call(document.createTextNode('x'),arg);}catch(e){failures++;}
        failures===2&&calls===0&&query.call(document.body,'p').length===1;
    "#,
    );
}

#[test]
fn numeric_names_and_reserved_methods_keep_distinct_named_lookup() {
    check(
        "<p id=one></p><p id=00></p><p id=4294967295></p><p id=toString></p>",
        r#"
        const nodes=document.getElementsByTagName('p');
        nodes['00']===nodes[1]&&nodes['4294967295']===nodes[2]
            &&nodes.namedItem('toString')===nodes[3]&&nodes.toString!==nodes[3]
            &&nodes['01']===undefined&&nodes['-1']===undefined;
    "#,
    );
}

#[test]
fn collections_survive_gc_and_reflect_fragment_replacement_without_reusing_old_nodes() {
    let mut s = session("<main id=root><p id=old></p></main>");
    s.execute("var nodes=document.getElementsByTagName('p');var old=nodes[0];")
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
    assert_eq!(s.execute("document.getElementById('root').innerHTML='<p id=new></p><p id=other></p>';nodes.length===2&&nodes[0].id==='new'&&nodes[0]!==old&&old.parentNode===null;").unwrap(),Value::Bool(true));
}

#[test]
fn loader_can_find_the_first_script_and_insert_before_it_with_a_live_collection() {
    check(
        "<script id=anchor type='application/json'></script>",
        r#"
        const scripts=document.getElementsByTagName('script');const anchor=scripts[0];
        const script=document.createElement('script');script.type='application/json';script.id='before';
        anchor.parentNode.insertBefore(script,anchor);
        scripts.length===2&&scripts[0]===script&&scripts[1]===anchor&&scripts.namedItem('before')===script;
    "#,
    );
}

#[test]
fn reentrant_tag_coercion_uses_post_conversion_query_storage() {
    let mut s = session("<p></p>");
    assert_eq!(s.execute("let inner;const outer=document.getElementsByTagName({toString:function(){inner=document.getElementsByTagName('p');return 'P';}});outer===inner&&outer.length===1;").unwrap(),Value::Bool(true));
    assert_eq!(s.state.collections.queries.len(), 1);
}

#[test]
fn fatal_rollback_keeps_existing_collections_but_restores_the_original_dom() {
    let mut s = session("<main id=root><p id=old></p></main>");
    s.execute("var nodes=document.getElementsByTagName('p');")
        .unwrap();
    let old = s
        .document
        .elements()
        .find_map(|(id, e)| (e.attr("id") == Some("old")).then_some(id))
        .unwrap();
    let error=s.execute("document.getElementById('root').innerHTML='<p id=new></p><p></p>';document.getElementsByTagName('main');while(true){}").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(s.scripts_disabled);
    assert_eq!(s.state.collections.queries.len(), 1);
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    assert_eq!(host.get(BASE, "length").unwrap(), Value::Number(1.0));
    assert_eq!(host.get(BASE, "0").unwrap(), DomHost::node_value(old));
    let mut runtime = Runtime::default();
    assert!(
        host.collection_call(
            METHOD,
            "call",
            &[Value::Number(0.0)],
            &Value::Host(BASE + 63),
            &mut runtime
        )
        .is_err()
    );
}

#[test]
fn unsupported_collection_writes_do_not_mutate_dom_or_collection_membership() {
    check(
        "<p id=one></p>",
        r#"
        const nodes=document.getElementsByTagName('p');let failures=0;
        try{nodes.length=0;}catch(e){failures++;}try{nodes[0]=null;}catch(e){failures++;}
        try{nodes.extra=7;}catch(e){failures++;}
        failures===3&&nodes.length===1&&nodes[0].id==='one'&&nodes.extra===undefined;
    "#,
    );
}
