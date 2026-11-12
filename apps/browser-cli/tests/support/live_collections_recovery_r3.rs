//! Local inline live-collection fixture in an actual retained worker.
use browser_ipc::{Color, DisplayItem, DocumentInput, RenderedPage, Viewport};
use browser_process::RendererSession;

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
fn recovery_r3_worker_keeps_live_handles_and_static_snapshot_without_replay() {
    let document = DocumentInput {
        // Synthetic inline fixture; no HTTP/file authority is requested.
        url: "noir:live-collections-recovery-r3".into(),
        html: include_str!("../../../../fixtures/live-collections-recovery-r3.html").into(),
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
    assert_eq!(initial.title, "R3 job");
    assert!(initial.animation_frame_pending);
    rows(&initial, &["A", "B", "C"], &["D", "E"]);
    let button = initial
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Control { id, value, .. } if value == "Edit" => Some(*id),
            _ => None,
        })
        .unwrap();
    let reflow = session.render(viewport, None).unwrap();
    assert_eq!(reflow.title, "R3 job");
    assert_eq!(reflow.metrics.worker_pid, pid);
    assert_eq!(reflow.items, initial.items);
    let frame = session.animation_frame(1., viewport).unwrap();
    assert_eq!(frame.title, "R3 frame");
    assert_eq!(frame.metrics.worker_pid, pid);
    assert!(!frame.animation_frame_pending);
    rows(&frame, &["D", "E", "B"], &["A", "C"]);
    let clicked = session
        .render(viewport, Some((button, "click".into())))
        .unwrap();
    assert_eq!(clicked.title, "R3 clicked");
    assert_eq!(clicked.metrics.worker_pid, pid);
    rows(&clicked, &["D", "A", "B"], &["C", "E"]);
    let idle = session.animation_frame(2., viewport).unwrap();
    assert_eq!(idle.title, "R3 clicked");
    assert_eq!(idle.metrics.worker_pid, pid);
    assert_eq!(idle.items, clicked.items);
}
