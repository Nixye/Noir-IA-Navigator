//! Real principal border boxes and explicit host view context, R5.
use super::*;
use browser_ipc::{Color, DisplayItem, ViewContext};
use web_javascript::{ErrorKind, Value};
fn viewport() -> Viewport {
    Viewport {
        width: 320.,
        height: 240.,
        view_context: Some(ViewContext::Origin),
    }
}
fn session(html: &str) -> RenderSession {
    RenderSession::new_with_viewport(
        &DocumentInput {
            url: "noir:bounding-panel-recovery-r5".into(),
            html: html.into(),
            images: vec![],
            scripts: vec![],
            stylesheets: vec![],
            audio_metadata: vec![],
        },
        viewport(),
    )
    .unwrap()
}
fn check(s: &mut RenderSession, source: &str) {
    assert_eq!(s.session.execute(source).unwrap(), Value::Bool(true));
}
fn anchored(page: &RenderedPage, width: f32) {
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    let find = |color| {
        page.items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Rect {
                    rect,
                    color: actual,
                } if *actual == color => Some(*rect),
                _ => None,
            })
            .unwrap()
    };
    let a = find(Color(18, 52, 86, 255));
    let p = find(Color(36, 104, 172, 255));
    assert_eq!((a.x, a.y, a.width, a.height), (28.5, 38.25, width, 34.));
    assert_eq!(
        (p.x, p.y, p.width, p.height),
        (a.x + width, a.y + 34., 48., 20.)
    );
}
#[test]
fn recovery_r5_popup_tracks_fractional_border_box_while_old_snapshot_stays_static() {
    let mut s = session(include_str!(
        "../../../fixtures/bounding-panel-recovery-r5.html"
    ));
    let initial = s.render(viewport()).unwrap();
    assert_eq!(initial.title, "R5 anchored");
    anchored(&initial, 90.);
    let frame = s.animation_frame(1., viewport()).unwrap();
    assert_eq!(frame.title, "R5 expanded");
    anchored(&frame, 110.);
    check(
        &mut s,
        "original.width===90&&original.x===28.5&&original.y===38.25;",
    );
    check(
        &mut s,
        "original.width=7;anchor.getBoundingClientRect().width===110;",
    );
    // Restore only the caller-owned snapshot before the fixture's click assertion.
    check(&mut s, "original.width=90;true;");
    let button = s.session.query_selector("#resize").unwrap().unwrap() as u64;
    let clicked = s.dispatch_event(button, "click", viewport()).unwrap();
    assert_eq!(clicked.title, "R5 restored");
    anchored(&clicked, 90.);
    let idle = s.animation_frame(2., viewport()).unwrap();
    assert_eq!(idle.items, clicked.items);
    check(&mut s, "jobs===1&&frames===1&&clicks===1;");
}
#[test]
fn recovery_r5_scroll_only_updates_reuse_document_geometry_and_unknown_refuses_existing_box() {
    let mut s = session(
        "<div id=box style='width:100px;height:40px;margin:7px;padding:3px;border:2px solid red'></div><div style='width:1000px;height:1200px'></div>",
    );
    check(
        &mut s,
        "var box=document.getElementById('box');var saved=box.getBoundingClientRect();saved.width===110&&saved.height===50;",
    );
    for n in 0..80 {
        let mut v = viewport();
        v.view_context = Some(ViewContext::Sampled {
            scroll_x: 0.,
            scroll_y: n as f32,
            clip_width: 320.,
            clip_height: 240.,
        });
        s.update_geometry(v).unwrap();
        check(
            &mut s,
            &format!(
                "var current=box.getBoundingClientRect();current.top===saved.top-{n}&&current.left===saved.left&&current.width===saved.width;"
            ),
        );
    }
    let unknown = Viewport {
        view_context: None,
        ..viewport()
    };
    s.update_geometry(unknown).unwrap();
    let error = s
        .session
        .execute("box.getBoundingClientRect();")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Runtime);
    s.update_geometry(viewport()).unwrap();
    check(&mut s, "box.getBoundingClientRect().top===saved.top;");
}
#[test]
fn recovery_r5_hidden_clipped_and_nobox_have_distinct_measurement_contracts() {
    let mut s = session(
        "<div style='width:20px;height:20px;overflow:hidden'><div id=large style='width:100px;height:40px;visibility:hidden'></div></div><div id=none style='display:none'></div><input id=hidden type=hidden><span id=inline>inline</span>",
    );
    check(
        &mut s,
        "var large=document.getElementById('large');var rect=large.getBoundingClientRect();rect.width===100&&rect.height===40;",
    );
    let error = s
        .session
        .execute("document.getElementById('inline').getBoundingClientRect();")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Runtime);
    s.update_geometry(Viewport {
        view_context: None,
        ..viewport()
    })
    .unwrap();
    check(
        &mut s,
        "var detached=document.createElement('div');[document.getElementById('none'),document.getElementById('hidden'),detached].every(function(node){var r=node.getBoundingClientRect();return r.x===0&&r.y===0&&r.left===0&&r.top===0&&r.right===0&&r.bottom===0&&r.width===0&&r.height===0;});",
    );
}
