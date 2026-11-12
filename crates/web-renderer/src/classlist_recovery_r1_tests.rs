//! New integration on the restored V13 base; local inline inputs only.
use super::*;
use browser_ipc::{Color, DisplayItem};
use web_javascript::Value;

fn session(html: &str) -> RenderSession {
    RenderSession::new(&DocumentInput {
        url: "noir:classlist-recovery-r1".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    })
    .unwrap()
}
fn check(s: &mut RenderSession, script: &str) {
    assert_eq!(s.session.execute(script).unwrap(), Value::Bool(true));
}
fn menu_frame(page: &RenderedPage, open: bool) {
    let color = if open {
        Color(18, 52, 86, 255)
    } else {
        Color(101, 67, 33, 255)
    };
    let width = if open { 120. } else { 40. };
    // V13 paints both the block and text-run backgrounds. The local diagnostic
    // found a 120x24 block plus a 40.816x24 Menu run, not two layout boxes.
    // Identify the CSS box by its full dimensions, not by primitive count.
    let rects: Vec<_> = page
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect {
                rect,
                color: actual,
            } if *actual == color && rect.width == width && rect.height == 24. => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].width, width);
    assert_eq!(rects[0].height, 24.);
    let absent = if open {
        Color(101, 67, 33, 255)
    } else {
        Color(18, 52, 86, 255)
    };
    assert!(!page.items.iter().any(|item| matches!(item,
        DisplayItem::Rect { color, .. } if *color == absent)));
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
}

#[test]
fn recovery_classlist_fixture_updates_css_frame_and_retains_handles_across_raf_and_click() {
    let mut s = session(include_str!("../../../fixtures/classlist-recovery-r1.html"));
    let initial = s.render(Viewport::default()).unwrap();
    assert_eq!(initial.title, "ready");
    menu_frame(&initial, true);
    assert!(initial.animation_frame_pending);
    assert!(initial.items.iter().any(|item| matches!(item,
        DisplayItem::Text { text, color: Color(0, 136, 68, 255), .. } if text == "Settings")));
    let closed = s.animation_frame(1., Viewport::default()).unwrap();
    assert_eq!(closed.title, "closed");
    menu_frame(&closed, false);
    assert!(!closed.animation_frame_pending);
    check(
        &mut s,
        "tokens===menu.classList&&tokens.value==='menu dark'&&frames===1&&jobs===1&&conversions===1;",
    );
    let button = s.session.query_selector("#toggle").unwrap().unwrap() as u64;
    let clicked = s
        .dispatch_event(button, "click", Viewport::default())
        .unwrap();
    assert_eq!(clicked.title, "clicked");
    menu_frame(&clicked, true);
    let idle = s.animation_frame(2., Viewport::default()).unwrap();
    assert_eq!(idle.title, "clicked");
    menu_frame(&idle, true);
    check(&mut s, "clicks===1&&frames===1&&conversions===1;");
}

#[test]
fn recovery_classlist_live_attribute_and_value_edits_refresh_css_without_new_handle() {
    let mut s = session(
        "<style>.box{width:40px;height:24px;background:#654321}.box.open{width:120px;background:#123456}</style><div id=box class=box>Box</div>",
    );
    check(
        &mut s,
        "var box=document.getElementById('box'),list=box.classList;box.setAttribute('class','box  open open');list===box.classList&&list.length===2&&list[0]==='box'&&list.item(1)==='open'&&list.item(2)===null;",
    );
    menu_frame(&s.render(Viewport::default()).unwrap(), true);
    check(
        &mut s,
        "list.value='box';box.className==='box'&&list.length===1&&!list.contains('open');",
    );
    menu_frame(&s.render(Viewport::default()).unwrap(), false);
    check(
        &mut s,
        "box.className='box open';list.contains('open')&&list.length===2;",
    );
    menu_frame(&s.render(Viewport::default()).unwrap(), true);
}

#[test]
fn recovery_classlist_invalid_variadic_token_keeps_dom_and_frame_then_valid_edit_recovers() {
    let mut s = session(
        "<style>.box{width:40px;height:24px;background:#654321}.box.open{width:120px;background:#123456}</style><div id=box class=box>Box</div>",
    );
    check(
        &mut s,
        "var box=document.getElementById('box'),list=box.classList;var rejected=false;try{list.add('open','bad token');}catch(e){rejected=true;}rejected&&box.className==='box'&&!list.contains('open');",
    );
    menu_frame(&s.render(Viewport::default()).unwrap(), false);
    check(
        &mut s,
        "var calls=0;list.add({toString:function(){calls++;box.setAttribute('class','box kept');return 'open';}});calls===1&&list.contains('kept')&&list.contains('open');",
    );
    menu_frame(&s.render(Viewport::default()).unwrap(), true);
}
