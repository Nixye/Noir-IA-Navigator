//! Integration regression sources, not execution evidence.
use super::*;

fn session() -> DocumentSession {
    DocumentSession::new(
        web_html::parse(
            "<main id=root><button id=a data-code=A>Alpha</button><button id=b data-code=B>Beta</button></main><p id=result>safe</p>",
        )
        .unwrap(),
    )
}

fn text(session: &mut DocumentSession, selector: &str) -> String {
    let id = session.query_selector(selector).unwrap().unwrap();
    session.document.text_content(id)
}

#[test]
fn node_list_maps_real_nodes_with_this_arg_into_an_array() {
    let mut session = session();
    session
        .execute(
            r#"
            const nodes = document.querySelectorAll('button');
            const values = Array.from(nodes, function(node, index) {
                return this.prefix + index + ':' + node.getAttribute('data-code') + ':' + node.textContent;
            }, {prefix:'item'});
            document.getElementById('result').textContent = Array.isArray(values) + ':' +
                values.map(function(value) { return value + '!'; }).join('|');
            "#,
        )
        .unwrap();
    assert_eq!(
        text(&mut session, "#result"),
        "true:item0:A:Alpha!|item1:B:Beta!"
    );
}

#[test]
fn node_list_snapshot_keeps_node_identity_but_reads_later_mutations() {
    let mut session = session();
    assert_eq!(
        session
            .execute(
                r#"
                const nodes = document.querySelectorAll('button');
                const second = document.getElementById('b');
                const values = Array.from(nodes, function(node, index) {
                    if (index === 0) {
                        second.textContent = 'Changed';
                        second.remove();
                        const added = document.createElement('button');
                        added.textContent = 'New';
                        document.getElementById('root').appendChild(added);
                    }
                    return node.textContent;
                });
                values.join('|') + ':' + nodes.length + ':' +
                    (Array.from(nodes)[1] === second) + ':' +
                    Array.from(document.querySelectorAll('button'), function(node) {
                        return node.textContent;
                    }).join('|');
                "#,
            )
            .unwrap(),
        Value::string("Alpha|Changed:2:true:Alpha|New")
    );
}

#[test]
fn empty_node_list_does_not_call_mapper_and_invalid_mapper_is_catchable() {
    let mut session = session();
    assert_eq!(
        session
            .execute(
                r#"
                let calls = 0;
                const empty = Array.from(document.querySelectorAll('.missing'), function() { calls++; });
                let errorName = '';
                try { Array.from(document.querySelectorAll('button'), 42); }
                catch (error) { errorName = error.name; }
                Array.isArray(empty) + ':' + empty.length + ':' + calls + ':' + errorName;
                "#,
            )
            .unwrap(),
        Value::string("true:0:0:TypeError")
    );
    assert_eq!(text(&mut session, "#a"), "Alpha");
    assert!(!session.scripts_disabled);
}

#[test]
fn ordinary_mapper_error_stops_mapping_but_keeps_prior_dom_effects() {
    let mut session = session();
    assert_eq!(
        session
            .execute(
                r#"
                let calls = 0;
                let completed = false;
                let caught = '';
                try {
                    Array.from(document.querySelectorAll('button'), function(node) {
                        calls++;
                        node.textContent = 'Changed';
                        throw 'stop';
                    });
                    completed = true;
                } catch (error) { caught = error; }
                calls + ':' + completed + ':' + caught;
                "#,
            )
            .unwrap(),
        Value::string("1:false:stop")
    );
    assert_eq!(text(&mut session, "#a"), "Changed");
    assert_eq!(text(&mut session, "#b"), "Beta");
    assert!(!session.scripts_disabled);
}

#[test]
fn resource_failure_in_mapper_rolls_back_dom_and_vm_and_disables_scripts() {
    let mut session = session();
    session.runtime = Runtime::new(web_javascript::Limits {
        instructions: 2000,
        ..web_javascript::Limits::default()
    });
    session.execute("var changed=0;").unwrap();
    let error = session
        .execute(
            r#"
            try {
                Array.from(document.querySelectorAll('button'), function(node) {
                    changed++;
                    node.textContent = 'bad';
                    while (true) { changed++; }
                });
            } catch (error) { document.getElementById('result').textContent = 'caught'; }
            "#,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(text(&mut session, "#a"), "Alpha");
    assert_eq!(text(&mut session, "#b"), "Beta");
    assert_eq!(text(&mut session, "#result"), "safe");
    assert_eq!(
        session.runtime.global_value("changed"),
        Some(Value::Number(0.))
    );
    assert!(session.scripts_disabled);
    assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
}
