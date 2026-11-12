//! Real retained worker integration for the new continuation from V13.
use browser_ipc::{Color, DisplayItem, DocumentInput, RenderedPage, Viewport};
use browser_process::RendererSession;

fn document(html: &str) -> DocumentInput {
    DocumentInput {
        // Synthetic inline fixture, with no remote or file-origin authority.
        url: "noir:classlist-recovery-r1".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    }
}
fn menu_frame(page: &RenderedPage, open: bool) {
    let expected = if open {
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
            DisplayItem::Rect { rect, color }
                if *color == expected && rect.width == width && rect.height == 24. =>
            {
                Some(rect)
            }
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
fn recovery_classlist_worker_retains_css_jobs_frames_and_click_without_replay() {
    let viewport = Viewport::default();
    let (mut session, initial) = RendererSession::spawn(
        super::executable(),
        document(include_str!(
            "../../../../fixtures/classlist-recovery-r1.html"
        )),
        viewport,
    )
    .unwrap();
    let pid = initial.metrics.worker_pid;
    assert!(pid > 0);
    assert_ne!(pid, std::process::id());
    assert_eq!(initial.title, "ready");
    assert!(initial.animation_frame_pending);
    menu_frame(&initial, true);
    let button = initial
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Control { id, value, .. } if value == "Toggle" => Some(*id),
            _ => None,
        })
        .unwrap();
    let reflow = session.render(viewport, None).unwrap();
    assert_eq!(reflow.title, "ready");
    assert_eq!(reflow.metrics.worker_pid, pid);
    menu_frame(&reflow, true);
    let closed = session.animation_frame(1., viewport).unwrap();
    assert_eq!(closed.title, "closed");
    assert_eq!(closed.metrics.worker_pid, pid);
    assert!(!closed.animation_frame_pending);
    menu_frame(&closed, false);
    let clicked = session
        .render(viewport, Some((button, "click".into())))
        .unwrap();
    assert_eq!(clicked.title, "clicked");
    assert_eq!(clicked.metrics.worker_pid, pid);
    menu_frame(&clicked, true);
    let idle = session.animation_frame(2., viewport).unwrap();
    assert_eq!(idle.title, "clicked");
    assert_eq!(idle.metrics.worker_pid, pid);
    assert!(!idle.animation_frame_pending);
    menu_frame(&idle, true);
}

#[test]
fn recovery_classlist_worker_invalid_token_is_atomic_and_later_raf_still_runs() {
    let html = "<style>.box{width:40px;height:24px;background:#654321}.box.open{width:120px;background:#123456}</style><div id=box class=box>Box</div><script>var box=document.getElementById('box'),list=box.classList;var caught=false;try{list.add('open','bad token');}catch(e){caught=true;}document.title=caught&&box.className==='box'?'rejected':'failed';requestAnimationFrame(function(){list.toggle('open',true);document.title=caught&&list.contains('open')?'recovered':'failed';});</script>";
    let (mut session, initial) =
        RendererSession::spawn(super::executable(), document(html), Viewport::default()).unwrap();
    assert_eq!(initial.title, "rejected");
    menu_frame(&initial, false);
    let recovered = session.animation_frame(1., Viewport::default()).unwrap();
    assert_eq!(recovered.title, "recovered");
    assert_eq!(recovered.metrics.worker_pid, initial.metrics.worker_pid);
    menu_frame(&recovered, true);
}
