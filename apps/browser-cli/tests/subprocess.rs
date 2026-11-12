use browser_ipc::{
    DocumentInput, PROTOCOL_VERSION, RenderRequest, RenderResponse, Viewport, read_message,
    write_message,
};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_browser-cli"))
}

fn worker_command() -> Command {
    let mut command = Command::new(executable());
    command.arg("--renderer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[test]
fn real_worker_renders_in_an_independent_process_and_exits() {
    let document = browser_core::load_document("noir:specimen").unwrap();
    let page =
        browser_process::render_in_subprocess(executable(), document, Viewport::default()).unwrap();
    assert_ne!(page.metrics.worker_pid, std::process::id());
    assert!(page.metrics.worker_pid > 0);
    assert!(page.metrics.resident_memory_bytes.is_some_and(|n| n > 0));
    assert!(page.metrics.node_count > 10);
    assert!(!page.items.is_empty());
    assert!(page.items.iter().any(|item| matches!(item, browser_ipc::DisplayItem::Image { asset_id, .. } if asset_id == "palette.png")));
}

#[test]
fn worker_returns_version_error_as_a_versioned_response() {
    let request = RenderRequest {
        session_storage: Default::default(),
        protocol_version: PROTOCOL_VERSION + 1,
        id: 987,
        document: DocumentInput {
            url: "noir:test".to_owned(),
            html: "<p>Test</p>".to_owned(),
            audio_metadata: vec![],
            images: vec![],
            scripts: vec![],
            stylesheets: vec![],
        },
        viewport: Viewport::default(),
    };
    let mut child = worker_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    write_message(&mut input, &request).unwrap();
    drop(input);
    let response: RenderResponse = read_message(&mut child.stdout.take().unwrap()).unwrap();
    assert_eq!(response.protocol_version, PROTOCOL_VERSION);
    assert_eq!(response.id, 987);
    assert!(response.result.is_err());
    assert!(child.wait().unwrap().success());
}

#[test]
fn truncated_or_oversized_protocol_causes_clean_worker_failure() {
    for bytes in [&[255, 255, 255, 255][..], &[10, 0, 0, 0, b'{'][..]] {
        let mut child = worker_command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("IPC request"));
    }
}

#[test]
fn unexpected_worker_exit_is_reported_without_hanging_the_caller() {
    // A Rust test harness rejects --renderer and exits. This exercises abrupt
    // worker termination without adding a non-product helper executable.
    let bad_worker = std::env::current_exe().unwrap();
    let document = browser_core::load_document("noir:welcome").unwrap();
    let start = Instant::now();
    assert!(
        browser_process::render_in_subprocess(&bad_worker, document, Viewport::default()).is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[cfg(windows)]
#[test]
fn appcontainer_proves_file_network_and_child_process_denial_before_remote_render() {
    browser_process::verify_remote_sandbox(executable()).unwrap();
    let document = DocumentInput {
        url: "https://example.com/controlled-test".into(),
        html:
            "<title>Sandboxed page</title><p>Remote document bytes without network in renderer</p>"
                .into(),
        audio_metadata: vec![],
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let page =
        browser_process::render_in_subprocess(executable(), document, Viewport::default()).unwrap();
    assert_eq!(page.title, "Sandboxed page");
    assert_ne!(page.metrics.worker_pid, std::process::id());
    assert!(
        page.metrics
            .resident_memory_bytes
            .is_some_and(|m| m > 0 && m < 512 * 1024 * 1024)
    );
}

#[test]
fn remote_origin_is_rejected_in_an_unsandboxed_worker() {
    let request = RenderRequest {
        session_storage: Default::default(),
        protocol_version: PROTOCOL_VERSION,
        id: 1234,
        document: DocumentInput {
            url: "https://example.com/".into(),
            html: "<p>Remote</p>".into(),
            audio_metadata: vec![],
            images: vec![],
            scripts: vec![],
            stylesheets: vec![],
        },
        viewport: Viewport::default(),
    };
    let mut child = worker_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    write_message(&mut child.stdin.take().unwrap(), &request).unwrap();
    let response: RenderResponse = read_message(&mut child.stdout.take().unwrap()).unwrap();
    assert!(response.result.unwrap_err().contains("AppContainer"));
    let _ = child.wait();
}

#[cfg(not(windows))]
#[test]
fn remote_navigation_fails_closed_without_an_os_sandbox_backend() {
    assert!(browser_process::verify_remote_sandbox(executable()).is_err());
}

#[test]
fn live_document_preserves_vm_globals_and_delivers_real_clicks_after_reflow() {
    let document=DocumentInput{url:"noir:session-test".into(),html:"<p id=value>0</p><button type=button id=increment>Increment</button><script>let count=0;const button=document.getElementById('increment');button.addEventListener('click',()=>{count++;document.getElementById('value').textContent=String(count);});</script>".into(),images:vec![],audio_metadata:vec![],scripts:vec![],stylesheets:vec![]};
    let (mut session, first) =
        browser_process::RendererSession::spawn(executable(), document, Viewport::default())
            .unwrap();
    let pid = first.metrics.worker_pid;
    let button = first
        .items
        .iter()
        .find_map(|item| match item {
            browser_ipc::DisplayItem::Control {
                id,
                kind: browser_ipc::ControlKind::Button,
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("native button from own layout");
    let one = session
        .render(Viewport::default(), Some((button, "click".into())))
        .unwrap();
    assert!(
        one.items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="1"))
    );
    let reflow = session
        .render(
            Viewport {
                width: 640.,
                height: 480.,
                view_context: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(reflow.metrics.worker_pid, pid);
    assert!(
        reflow
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="1"))
    );
    let two = session
        .render(Viewport::default(), Some((button, "click".into())))
        .unwrap();
    assert_eq!(two.metrics.worker_pid, pid);
    assert!(
        two.items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="2"))
    );
}

#[cfg(windows)]
#[test]
fn remote_live_document_retains_state_inside_the_verified_appcontainer() {
    let document=DocumentInput{url:"https://example.com/stateful-test".into(),html:"<p id=value>Before</p><button type=button id=change>Change</button><script>document.getElementById('change').addEventListener('click',()=>{document.getElementById('value').textContent='After';});</script>".into(),images:vec![],audio_metadata:vec![],scripts:vec![],stylesheets:vec![]};
    let (mut session, first) =
        browser_process::RendererSession::spawn(executable(), document, Viewport::default())
            .unwrap();
    let button = first
        .items
        .iter()
        .find_map(|item| match item {
            browser_ipc::DisplayItem::Control {
                id,
                kind: browser_ipc::ControlKind::Button,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    let after = session
        .render(Viewport::default(), Some((button, "click".into())))
        .unwrap();
    assert_eq!(after.metrics.worker_pid, first.metrics.worker_pid);
    assert!(
        after
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="After"))
    );
}

#[test]
fn typed_native_values_reach_the_existing_vm_as_data_before_click() {
    let document=DocumentInput{url:"noir:input-session".into(),html:"<input id=name value=Initial><p id=result>Before</p><button type=button id=read>Read</button><script>document.getElementById('read').addEventListener('click',()=>{document.getElementById('result').textContent=document.getElementById('name').value;});</script>".into(),images:vec![],audio_metadata:vec![],scripts:vec![],stylesheets:vec![]};
    let (mut session, page) =
        browser_process::RendererSession::spawn(executable(), document, Viewport::default())
            .unwrap();
    let input = page
        .items
        .iter()
        .find_map(|item| match item {
            browser_ipc::DisplayItem::Control {
                id,
                kind: browser_ipc::ControlKind::Text,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    let button = page
        .items
        .iter()
        .find_map(|item| match item {
            browser_ipc::DisplayItem::Control {
                id,
                kind: browser_ipc::ControlKind::Button,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    let updated = session
        .event_with_inputs(
            Viewport::default(),
            button,
            "click".into(),
            vec![(input, "Ação local".into())],
        )
        .unwrap();
    assert!(
        updated
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="Ação"))
    );
    assert!(
        updated
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="local"))
    );
    assert_eq!(updated.metrics.worker_pid, page.metrics.worker_pid);
}

#[test]
fn retained_delayed_timer_runs_only_when_its_document_clock_is_due() {
    let document=DocumentInput{url:"noir:timer-session".into(),html:"<p id=result>Before</p><script>setTimeout(()=>{document.getElementById('result').textContent='After';},50);</script>".into(),images:vec![],audio_metadata:vec![],scripts:vec![],stylesheets:vec![]};
    let (mut session, page) =
        browser_process::RendererSession::spawn(executable(), document, Viewport::default())
            .unwrap();
    assert_eq!(page.next_timer_ms, Some(50));
    let not_yet = session.advance_timers(Viewport::default(), 49).unwrap();
    assert_eq!(not_yet.next_timer_ms, Some(1));
    assert!(
        not_yet
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="Before"))
    );
    assert!(session.advance_timers(Viewport::default(), 60_001).is_err());
    let after = session.advance_timers(Viewport::default(), 1).unwrap();
    assert_eq!(after.next_timer_ms, None);
    assert_eq!(after.metrics.worker_pid, page.metrics.worker_pid);
    assert!(
        after
            .items
            .iter()
            .any(|item| matches!(item,browser_ipc::DisplayItem::Text{text,..} if text=="After"))
    );
}

fn assert_retained_animation_state(
    page: &browser_ipc::RenderedPage,
    worker_pid: u32,
    expected: &str,
    pending: bool,
    next_timer_ms: Option<u64>,
) {
    assert_eq!(page.metrics.worker_pid, worker_pid);
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    assert_eq!(page.title, expected);
    assert!(
        page.items.iter().any(
            |item| matches!(item, browser_ipc::DisplayItem::Text { text, .. } if text == expected)
        ),
        "expected display text {expected:?}, got {:?}",
        page.items
    );
    assert_eq!(page.animation_frame_pending, pending);
    assert_eq!(page.next_timer_ms, next_timer_ms);
}

#[test]
fn retained_animation_frames_preserve_batch_order_and_do_not_advance_timers() {
    let document = DocumentInput {
        url: "noir:animation-session".into(),
        html: r#"<title>pending</title><p id=result>pending</p><script>
            var seen = '';
            function record(value) {
                seen += value;
                document.title = seen;
                document.getElementById('result').textContent = seen;
            }
            var canceled = requestAnimationFrame(() => { record('canceled'); });
            cancelAnimationFrame(canceled);
            requestAnimationFrame(t => {
                record('a' + t);
                cancelAnimationFrame(later);
                Promise.resolve().then(() => { record('m'); });
                requestAnimationFrame(t => { record('c' + t); });
            });
            var later = requestAnimationFrame(() => { record('canceled-later'); });
            requestAnimationFrame(t => { record('b' + t); });
            setTimeout(() => { record('t'); }, 50);
        </script>"#
            .into(),
        audio_metadata: vec![],
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let viewport = Viewport::default();
    let (mut session, initial) =
        browser_process::RendererSession::spawn(executable(), document, viewport).unwrap();
    let pid = initial.metrics.worker_pid;
    assert_ne!(pid, std::process::id());
    assert!(pid > 0);
    assert_retained_animation_state(&initial, pid, "pending", true, Some(50));

    let resized = Viewport {
        width: 640.,
        height: 480.,
        view_context: None,
    };
    let reflow = session.render(resized, None).unwrap();
    assert_retained_animation_state(&reflow, pid, "pending", true, Some(50));
    let tick = session.advance_timers(resized, 0).unwrap();
    assert_retained_animation_state(&tick, pid, "pending", true, Some(50));

    let first = session.animation_frame(12.5, resized).unwrap();
    assert_retained_animation_state(&first, pid, "a12.5mb12.5", true, Some(50));
    let reflow = session.render(viewport, None).unwrap();
    assert_retained_animation_state(&reflow, pid, "a12.5mb12.5", true, Some(50));

    // This timestamp is past the timer's delay, but only the explicit timer
    // clock below may fire that callback. The newly registered frame runs now.
    let second = session.animation_frame(1000., viewport).unwrap();
    assert_retained_animation_state(&second, pid, "a12.5mb12.5c1000", false, Some(50));
    let idle = session.animation_frame(1001., viewport).unwrap();
    assert_retained_animation_state(&idle, pid, "a12.5mb12.5c1000", false, Some(50));
    let not_due = session.advance_timers(viewport, 49).unwrap();
    assert_retained_animation_state(&not_due, pid, "a12.5mb12.5c1000", false, Some(1));
    let due = session.advance_timers(viewport, 1).unwrap();
    assert_retained_animation_state(&due, pid, "a12.5mb12.5c1000t", false, None);
}

#[test]
fn retained_animation_frames_reject_bad_timestamps_without_losing_the_session() {
    let document = DocumentInput {
        url: "noir:animation-timestamp-session".into(),
        html: r#"<title>pending</title><p id=result>pending</p><script>
            requestAnimationFrame(t => {
                document.title = 'first' + t;
                document.getElementById('result').textContent = document.title;
                requestAnimationFrame(t => {
                    document.title = 'second' + t;
                    document.getElementById('result').textContent = document.title;
                });
            });
        </script>"#
            .into(),
        audio_metadata: vec![],
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let viewport = Viewport::default();
    let (mut session, initial) =
        browser_process::RendererSession::spawn(executable(), document, viewport).unwrap();
    let pid = initial.metrics.worker_pid;
    assert_ne!(pid, std::process::id());
    assert!(pid > 0);
    assert_retained_animation_state(&initial, pid, "pending", true, None);

    for timestamp in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.,
        browser_ipc::MAX_ANIMATION_FRAME_TIMESTAMP_MS + 1.,
    ] {
        let error = session.animation_frame(timestamp, viewport).unwrap_err();
        assert!(error.contains("timestamp"), "{timestamp:?}: {error}");
        let unchanged = session.render(viewport, None).unwrap();
        assert_retained_animation_state(&unchanged, pid, "pending", true, None);
    }

    let first = session.animation_frame(12.5, viewport).unwrap();
    assert_retained_animation_state(&first, pid, "first12.5", true, None);
    let error = session.animation_frame(12., viewport).unwrap_err();
    assert!(error.contains("timestamp"), "{error}");
    let unchanged = session.render(viewport, None).unwrap();
    assert_retained_animation_state(&unchanged, pid, "first12.5", true, None);

    // Equal accepted timestamps are valid and must retain the queued callback.
    let second = session.animation_frame(12.5, viewport).unwrap();
    assert_retained_animation_state(&second, pid, "second12.5", false, None);
    let idle = session.animation_frame(13., viewport).unwrap();
    assert_retained_animation_state(&idle, pid, "second12.5", false, None);
}

#[path = "support/classlist_recovery_r1.rs"]
mod classlist_recovery_r1;

#[path = "support/background_ownership_recovery_r2.rs"]
mod background_ownership_recovery_r2;

#[path = "support/live_collections_recovery_r3.rs"]
mod live_collections_recovery_r3;

#[path = "support/nodelist_foreach_recovery_r4.rs"]
mod nodelist_foreach_recovery_r4;

#[path = "support/bounding_panel_recovery_r5.rs"]
mod bounding_panel_recovery_r5;
