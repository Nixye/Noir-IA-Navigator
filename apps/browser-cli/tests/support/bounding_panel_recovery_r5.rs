//! Retained worker measurements, explicit context and offline script continuation.
use browser_ipc::{Color, DisplayItem, DocumentInput, RenderedPage, ViewContext, Viewport};
use browser_process::RendererSession;
fn viewport() -> Viewport {
    Viewport {
        width: 320.,
        height: 240.,
        view_context: Some(ViewContext::Origin),
    }
}
fn document(html: &str) -> DocumentInput {
    DocumentInput {
        url: "noir:bounding-panel-recovery-r5".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    }
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
fn recovery_r5_worker_popup_retains_measurement_snapshots_and_callbacks_without_replay() {
    let v = viewport();
    let (mut s, initial) = RendererSession::spawn(
        super::executable(),
        document(include_str!(
            "../../../../fixtures/bounding-panel-recovery-r5.html"
        )),
        v,
    )
    .unwrap();
    let pid = initial.metrics.worker_pid;
    assert!(pid > 0);
    assert_ne!(pid, std::process::id());
    assert_eq!(initial.title, "R5 anchored");
    anchored(&initial, 90.);
    let reflow = s.render(v, None).unwrap();
    assert_eq!(reflow.title, "R5 anchored");
    assert_eq!(reflow.metrics.worker_pid, pid);
    let frame = s.animation_frame(1., v).unwrap();
    assert_eq!(frame.title, "R5 expanded");
    assert_eq!(frame.metrics.worker_pid, pid);
    anchored(&frame, 110.);
    let button = frame
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Control { id, value, .. } if value == "Resize" => Some(*id),
            _ => None,
        })
        .unwrap();
    let clicked = s.render(v, Some((button, "click".into()))).unwrap();
    assert_eq!(clicked.title, "R5 restored");
    assert_eq!(clicked.metrics.worker_pid, pid);
    anchored(&clicked, 90.);
    let idle = s.animation_frame(2., v).unwrap();
    assert_eq!(idle.title, "R5 restored");
    assert_eq!(idle.metrics.worker_pid, pid);
    assert_eq!(idle.items, clicked.items);
}
#[test]
fn recovery_r5_worker_scripts_and_raf_install_known_or_unknown_context_before_callbacks() {
    use browser_ipc::{
        DynamicScriptResponse, PROTOCOL_VERSION, RenderRequest, RenderResponse, SessionRequest,
        read_message, write_message,
    };
    use std::process::{Command, Stdio};
    struct Cleanup(std::process::Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Cleanup(
        Command::new(super::executable())
            .arg("--renderer-session")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let mut output = child.0.stdout.take().unwrap();
    let html = "<div id=box style='width:100px;height:40px'></div><div style='height:1000px'></div><script>var box=document.getElementById('box');var original=box.getBoundingClientRect();var calls=0;var script=document.createElement('script');script.src='later.js';document.head.appendChild(script);</script>";
    write_message(
        &mut input,
        &SessionRequest::Begin(RenderRequest {
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            document: document(html),
            viewport: viewport(),
            session_storage: Default::default(),
        }),
    )
    .unwrap();
    let first: RenderResponse = read_message(&mut output).unwrap();
    let initial = first.result.unwrap();
    let pid = initial.metrics.worker_pid;
    assert!(pid > 0);
    assert_eq!(first.pending_scripts.len(), 1);
    let sampled = Viewport {
        view_context: Some(ViewContext::Sampled {
            scroll_x: 0.,
            scroll_y: 25.,
            clip_width: 320.,
            clip_height: 240.,
        }),
        ..viewport()
    };
    write_message(&mut input, &SessionRequest::Scripts { id: 2, scripts: vec![DynamicScriptResponse { id: first.pending_scripts[0].id, source: Some("calls++;document.title=calls===1&&box.getBoundingClientRect().top===original.top-25?'R5 script scrolled':'R5 failed';requestAnimationFrame(function(){var refused=false;try{box.getBoundingClientRect();}catch(e){refused=true;}document.title=refused&&calls===1?'R5 unknown refused':'R5 failed';});".into()) }], viewport: sampled }).unwrap();
    let second: RenderResponse = read_message(&mut output).unwrap();
    let page = second.result.unwrap();
    assert_eq!(page.title, "R5 script scrolled");
    assert_eq!(page.metrics.worker_pid, pid);
    assert!(page.warnings.is_empty());
    assert!(second.pending_scripts.is_empty());
    write_message(
        &mut input,
        &SessionRequest::AnimationFrame {
            id: 3,
            timestamp_ms: 1.,
            viewport: Viewport {
                view_context: None,
                ..viewport()
            },
        },
    )
    .unwrap();
    let third: RenderResponse = read_message(&mut output).unwrap();
    let page = third.result.unwrap();
    assert_eq!(page.title, "R5 unknown refused");
    assert_eq!(page.metrics.worker_pid, pid);
    assert!(page.warnings.is_empty());
    drop(input);
    assert!(child.0.wait().unwrap().success());
}
