//! R4 NodeList initialization through real DOM/VM/layout.
use super::*;
use browser_ipc::{Color, DisplayItem};
use web_javascript::Value;

fn session(html: &str) -> RenderSession {
    RenderSession::new(&DocumentInput {
        url: "noir:nodelist-foreach-recovery-r4".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    })
    .unwrap()
}
fn card(page: &RenderedPage, label: &str, selected: bool) {
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    let text = page
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text { text, rect, .. } if text == label => Some(*rect),
            _ => None,
        })
        .unwrap();
    let expected = if selected {
        Color(0, 136, 68, 255)
    } else {
        Color(36, 104, 172, 255)
    };
    assert!(page.items.iter().any(|item| matches!(item,
        DisplayItem::Rect { rect, color } if *color == expected && rect.x == text.x && rect.y == text.y && rect.width == 240. && rect.height == 48.)));
}
#[test]
fn recovery_r4_foreach_initializes_filters_with_thisarg_then_retains_events_and_frames() {
    let mut s = session(include_str!(
        "../../../fixtures/nodelist-foreach-recovery-r4.html"
    ));
    let initial = s.render(Viewport::default()).unwrap();
    assert_eq!(initial.title, "R4 ready");
    card(&initial, "Alpha", false);
    card(&initial, "Beta", false);
    let frame = s.animation_frame(1., Viewport::default()).unwrap();
    assert_eq!(frame.title, "R4 frame");
    card(&frame, "Alpha", false);
    card(&frame, "Beta", true);
    let button = s.session.query_selector("#picka").unwrap().unwrap() as u64;
    let clicked = s
        .dispatch_event(button, "click", Viewport::default())
        .unwrap();
    assert_eq!(clicked.title, "R4 clicked0");
    card(&clicked, "Alpha", true);
    card(&clicked, "Beta", false);
    let idle = s.animation_frame(2., Viewport::default()).unwrap();
    assert_eq!(idle.items, clicked.items);
    assert_eq!(idle.title, "R4 clicked0");
    assert_eq!(s.session.execute("context.initialized===2&&indices==='01'&&receiverCorrect&&jobs===1&&frames===1&&clicks===1;").unwrap(), Value::Bool(true));
}
#[test]
fn recovery_r4_live_iteration_uses_current_indices_but_snapshot_keeps_removed_nodes() {
    let mut s =
        session("<div id=root><span id=a>A</span><span id=b>B</span><span id=c>C</span></div>");
    assert_eq!(s.session.execute("var root=document.getElementById('root'),a=document.getElementById('a'),b=document.getElementById('b'),c=document.getElementById('c');var live=root.childNodes,snapshot=document.querySelectorAll('span');var d=document.createElement('span');d.id='d';d.textContent='D';var seen='';live.forEach(function(node,index,receiver){seen+=node.id+index;if(index===0){root.appendChild(d);root.removeChild(b);}if(receiver!==live){throw 'receiver';}});seen==='a0c1d2'&&live.length===3&&snapshot.length===3;").unwrap(), Value::Bool(true));
    assert_eq!(s.session.execute("var kept='';snapshot.forEach(function(node,index,receiver){kept+=node.id+index;if(receiver!==snapshot){throw 'snapshot receiver';}});kept==='a0b1c2'&&snapshot.item(1)===b&&b.parentNode===null&&live.item(1)===c;").unwrap(), Value::Bool(true));
    assert_eq!(s.session.execute("var limited='';live.forEach(function(node,index){limited+=node.id;if(index===0){var e=document.createElement('span');e.id='e';e.textContent='E';root.appendChild(e);}});limited==='acd'&&live.length===4;").unwrap(), Value::Bool(true));
    let page = s.render(Viewport::default()).unwrap();
    assert!(page.warnings.is_empty());
    for label in ["A", "C", "D", "E"] {
        assert!(
            page.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == label))
        );
    }
    assert!(
        !page
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == "B"))
    );
}
