//! Live collections through retained DOM/VM and real layout, authored for R3.
use super::*;
use browser_ipc::{Color, DisplayItem};
use web_javascript::Value;

fn session(html: &str) -> RenderSession {
    RenderSession::new(&DocumentInput {
        url: "noir:live-collections-recovery-r3".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    })
    .unwrap()
}
fn rows(page: &RenderedPage, expected: &[&str], absent: &[&str]) {
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    for label in expected {
        let text: Vec<_> = page
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, rect, .. } if text.as_str() == *label => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(text.len(), 1, "row {label}");
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Rect { rect, color: Color(36, 104, 172, 255) }
                if rect.x == text[0].x && rect.y == text[0].y && rect.width == 120. && rect.height == 32.)));
    }
    for label in absent {
        assert!(
            !page.items.iter().any(|item| matches!(item,
            DisplayItem::Text { text, .. } if text.as_str() == *label)),
            "stale row {label}"
        );
    }
}

#[test]
fn recovery_r3_retained_collections_follow_job_raf_click_while_query_snapshot_stays_static() {
    let mut s = session(include_str!(
        "../../../fixtures/live-collections-recovery-r3.html"
    ));
    let initial = s.render(Viewport::default()).unwrap();
    assert_eq!(initial.title, "R3 job");
    rows(&initial, &["A", "B", "C"], &["D", "E"]);
    let frame = s.animation_frame(1., Viewport::default()).unwrap();
    assert_eq!(frame.title, "R3 frame");
    rows(&frame, &["D", "E", "B"], &["A", "C"]);
    let button = s.session.query_selector("#edit").unwrap().unwrap() as u64;
    let clicked = s
        .dispatch_event(button, "click", Viewport::default())
        .unwrap();
    assert_eq!(clicked.title, "R3 clicked");
    rows(&clicked, &["D", "A", "B"], &["C", "E"]);
    let idle = s.animation_frame(2., Viewport::default()).unwrap();
    assert_eq!(idle.title, "R3 clicked");
    assert_eq!(idle.items, clicked.items);
    assert_eq!(s.session.execute("stable&&jobs===1&&frames===1&&clicks===1&&elements===list.children&&nodes===list.childNodes;").unwrap(), Value::Bool(true));
}

#[test]
fn recovery_r3_named_children_refresh_after_rename_and_reparent_but_nodes_have_no_names() {
    let mut s = session(
        "<div id=left>text<span id=first name=choice>A</span><span id=last>B</span></div><div id=right></div>",
    );
    assert_eq!(s.session.execute("var left=document.getElementById('left'),right=document.getElementById('right'),first=document.getElementById('first'),last=document.getElementById('last');var children=left.children,nodes=left.childNodes,snapshot=document.querySelectorAll('span');children.namedItem('choice')===first&&children.first===first&&children.length===2&&nodes.length===3&&nodes.item(1)===first&&nodes.choice===undefined;").unwrap(), Value::Bool(true));
    assert_eq!(s.session.execute("first.setAttribute('name','renamed');first.id='changed';right.appendChild(last);children.namedItem('choice')===null&&children.namedItem('renamed')===first&&children.changed===first&&children.length===1&&children.item(1)===null&&nodes.length===2&&snapshot.length===2&&snapshot[1]===last;").unwrap(), Value::Bool(true));
    assert_eq!(s.session.execute("left.innerHTML='<span id=new>New</span>';children===left.children&&nodes===left.childNodes&&children.length===1&&nodes.length===1&&children[0].id==='new'&&nodes[0]===children[0]&&children.namedItem('changed')===null&&snapshot[0]===first&&first.parentNode===null;").unwrap(), Value::Bool(true));
    let page = s.render(Viewport::default()).unwrap();
    assert!(page.warnings.is_empty());
    assert!(
        page.items
            .iter()
            .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == "New"))
    );
    assert!(
        !page
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == "A"))
    );
}
