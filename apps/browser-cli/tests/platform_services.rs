//! Real worker integration of OS-backed entropy and document-clock injection.
use browser_ipc::{DocumentInput, Viewport};
use std::path::Path;

fn executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_browser-cli"))
}
fn document(url: &str) -> DocumentInput {
    DocumentInput {
        url: url.into(),
        html: r#"<title>not executed</title><script>
        const origin = performance.timeOrigin;
        const before = performance.now();
        const random = Math.random();
        const valid = performance === window.performance && before >= 0
            && origin > 0 && random >= 0 && random < 1;
        document.title = valid ? 'platform ready' : 'platform failed';
        setTimeout(function() {
            const later = performance.now();
            const next = Math.random();
            document.title = valid && later >= before && performance.timeOrigin === origin
                && next >= 0 && next < 1 ? 'platform callback ready' : 'platform callback failed';
        }, 10);
        </script>"#
            .into(),
        images: vec![],
        audio_metadata: vec![],
        scripts: vec![],
        stylesheets: vec![],
    }
}

#[test]
fn one_shot_worker_installs_real_platform_services_before_initial_scripts() {
    let page = browser_process::render_in_subprocess(
        executable(),
        document("noir:platform-test"),
        Viewport::default(),
    )
    .unwrap();
    assert_eq!(page.title, "platform ready");
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    assert_ne!(page.metrics.worker_pid, std::process::id());
}

#[test]
fn retained_worker_keeps_real_services_for_later_callbacks_and_reflow() {
    let viewport = Viewport::default();
    let (mut session, first) = browser_process::RendererSession::spawn(
        executable(),
        document("noir:platform-test"),
        viewport,
    )
    .unwrap();
    assert_eq!(first.title, "platform ready");
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let callback = session.advance_timers(viewport, 10).unwrap();
    assert_eq!(callback.title, "platform callback ready");
    assert!(callback.warnings.is_empty(), "{:?}", callback.warnings);
    let reflow = session.render(viewport, None).unwrap();
    assert_eq!(reflow.title, callback.title);
    assert_eq!(reflow.metrics.worker_pid, first.metrics.worker_pid);
}

#[cfg(windows)]
#[test]
fn appcontainer_worker_can_use_platform_services_without_new_capabilities() {
    // Synthetic controlled bytes, not a network fetch or replay of a live site.
    // render_in_subprocess still proves the normal AppContainer denial policy.
    let page = browser_process::render_in_subprocess(
        executable(),
        document("https://example.com/controlled-platform-test"),
        Viewport::default(),
    )
    .unwrap();
    assert_eq!(page.title, "platform ready");
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
}
