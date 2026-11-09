//! Regression sources only; these tests were not executed for this change.
use super::*;

fn document() -> Document {
    web_html::parse("<main id=root><p id=a class='card x:y' data-code='A,B'>A</p>text<span></span><p id=b class=card data-code=B>B</p></main>").unwrap()
}
fn id(document: &Document, name: &str) -> NodeId {
    document
        .elements()
        .find(|(_, element)| element.attr("id") == Some(name))
        .unwrap()
        .0
}

#[test]
fn compound_lists_combinators_attributes_and_escapes_query_real_dom() {
    let document = document();
    let a = id(&document, "a");
    let b = id(&document, "b");
    for selector in [
        "p#a.card",
        "body > main > p#a",
        "main .x\\:y",
        "[data-code='A,B']",
        r"#\61",
        "p:first-child:not([hidden])",
        ":is(#a, .missing)",
    ] {
        assert_eq!(
            query(&document, 0, selector, &mut 0, false).unwrap(),
            vec![a],
            "{selector}"
        );
    }
    for selector in ["span + p", "#a ~ p", "p:nth-child(3)", "[data-code=b i]"] {
        assert_eq!(
            query(&document, 0, selector, &mut 0, false).unwrap(),
            vec![b],
            "{selector}"
        );
    }
    assert_eq!(
        query(&document, 0, "#b, .card, #a, p", &mut 0, false).unwrap(),
        vec![a, b]
    );
    assert_eq!(
        query(&document, 0, "#b, #a", &mut 0, true).unwrap(),
        vec![a]
    );
}

#[test]
fn element_queries_exclude_root_but_match_ancestors_outside_it() {
    let document = document();
    let root = id(&document, "root");
    assert!(
        query(&document, root, "#root", &mut 0, false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        query(&document, root, "body main > p", &mut 0, false).unwrap(),
        vec![id(&document, "a"), id(&document, "b")]
    );
}

#[test]
fn detached_matches_closest_and_snapshots_preserve_node_identity() {
    let mut session = DocumentSession::new(document());
    assert_eq!(
        session
            .execute(
                r#"
        const root=document.getElementById('root');
        const a=document.getElementById('a');
        const b=document.getElementById('b');
        const snapshot=root.querySelectorAll('.card, #a');
        b.remove();
        a.setAttribute('class','changed');
        const fresh=root.querySelectorAll('.card');
        root.remove();
        snapshot.length===2 && snapshot[0]===a && snapshot[1]===b && fresh.length===0 &&
        a.matches('main > p.changed') && a.closest('main#root')===root &&
        a.closest('p.changed')===a && a.closest('body')===null &&
        b.matches('p.card') && b.closest('p.card')===b && b.closest('main')===null &&
        root.querySelector('p.changed')===a;
    "#
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn invalid_whole_lists_are_errors_even_when_first_branch_matches() {
    let document = document();
    for selector in [
        "",
        "p,",
        "p,,span",
        "p, [",
        "p, :scope",
        "p, ::before",
        "p, svg|rect",
        "p, :has(span)",
        "p, :is(.card,)",
    ] {
        for first in [false, true] {
            let mut work = 0;
            let error = query(&document, 0, selector, &mut work, first).unwrap_err();
            assert_ne!(error.kind, ErrorKind::Resource, "{selector}");
            assert!(work > 0);
        }
        assert!(selector_ancestor(&document, id(&document, "a"), selector, &mut 0, false).is_err());
        assert!(selector_ancestor(&document, id(&document, "a"), selector, &mut 0, true).is_err());
    }
    assert_eq!(
        query(&document, 0, &"p,".repeat(65), &mut 0, true)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn traversal_matching_and_errors_debit_one_cumulative_budget() {
    let document = web_html::parse(&format!("<main>{}</main>", "<p></p>".repeat(100))).unwrap();
    let mut first_work = 0;
    query(&document, 0, "*", &mut first_work, true).unwrap();
    let mut all_work = 0;
    query(&document, 0, "*", &mut all_work, false).unwrap();
    assert!(all_work > first_work);
    let mut near_limit = SELECTOR_READ_LIMIT - first_work;
    query(&document, 0, "*", &mut near_limit, true).unwrap();
    assert_eq!(near_limit, SELECTOR_READ_LIMIT);
    assert_eq!(
        query(&document, 0, "*", &mut near_limit, true)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    let mut insufficient = SELECTOR_READ_LIMIT - first_work;
    assert_eq!(
        query(&document, 0, "*", &mut insufficient, false)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(insufficient, SELECTOR_READ_LIMIT);
    let mut syntax_work = 12;
    assert!(query(&document, 0, "p, [", &mut syntax_work, true).is_err());
    assert!(syntax_work > 12);
    let mut parse_limit = SELECTOR_READ_LIMIT - 1;
    assert_eq!(
        query(&document, 0, "p", &mut parse_limit, true)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(parse_limit, SELECTOR_READ_LIMIT);
}

#[test]
fn inert_roots_are_visible_but_their_descendants_are_not_query_candidates() {
    let document=web_html::parse("<template id=t><p class=hidden></p></template><noscript id=n><p class=hidden></p></noscript><p id=visible></p>").unwrap();
    assert_eq!(
        query(&document, 0, "template, noscript", &mut 0, false).unwrap(),
        vec![id(&document, "t"), id(&document, "n")]
    );
    assert!(
        query(&document, 0, ".hidden", &mut 0, false)
            .unwrap()
            .is_empty()
    );
    for root in [id(&document, "t"), id(&document, "n")] {
        assert!(
            query(&document, root, "*", &mut 0, false)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn selector_resource_failure_rolls_back_dom_and_disables_scripts() {
    let mut session = DocumentSession::new(document());
    let selector = vec!["p"; 65].join(",");
    let error=session.execute(&format!("document.getElementById('a').setAttribute('data-code','changed');try{{document.querySelector('{selector}');}}catch(e){{}}")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(
        session
            .document
            .element(id(&session.document, "a"))
            .unwrap()
            .attr("data-code"),
        Some("A,B")
    );
    assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
}

#[test]
fn query_all_retains_node_list_quota() {
    let mut session = DocumentSession::new(web_html::parse(&"<p></p>".repeat(4097)).unwrap());
    assert_eq!(
        session
            .execute("document.querySelectorAll('p');")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}
