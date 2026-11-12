//! Real worker for NodeList-based filter initialization; no network or GUI.
use browser_ipc::{Color, DisplayItem, DocumentInput, RenderedPage, Viewport};
use browser_process::RendererSession;
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
fn recovery_r4_worker_foreach_handlers_thisarg_and_raf_survive_reflow_without_replay() {
    let document = DocumentInput {
        url: "noir:nodelist-foreach-recovery-r4".into(),
        html: include_str!("../../../../fixtures/nodelist-foreach-recovery-r4.html").into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    };
    let viewport = Viewport::default();
    let (mut session, initial) =
        RendererSession::spawn(super::executable(), document, viewport).unwrap();
    let pid = initial.metrics.worker_pid;
    assert!(pid > 0);
    assert_ne!(pid, std::process::id());
    assert_eq!(initial.title, "R4 ready");
    card(&initial, "Alpha", false);
    card(&initial, "Beta", false);
    assert!(initial.animation_frame_pending);
    let button = initial
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Control { id, value, .. } if value == "ChooseAlpha" => Some(*id),
            _ => None,
        })
        .unwrap();
    let reflow = session.render(viewport, None).unwrap();
    assert_eq!(reflow.metrics.worker_pid, pid);
    assert_eq!(reflow.items, initial.items);
    assert_eq!(reflow.title, "R4 ready");
    let frame = session.animation_frame(1., viewport).unwrap();
    assert_eq!(frame.metrics.worker_pid, pid);
    assert_eq!(frame.title, "R4 frame");
    card(&frame, "Alpha", false);
    card(&frame, "Beta", true);
    let clicked = session
        .render(viewport, Some((button, "click".into())))
        .unwrap();
    assert_eq!(clicked.metrics.worker_pid, pid);
    assert_eq!(clicked.title, "R4 clicked0");
    card(&clicked, "Alpha", true);
    card(&clicked, "Beta", false);
    let idle = session.animation_frame(2., viewport).unwrap();
    assert_eq!(idle.metrics.worker_pid, pid);
    assert_eq!(idle.title, "R4 clicked0");
    assert_eq!(idle.items, clicked.items);
    assert!(!idle.animation_frame_pending);
}
