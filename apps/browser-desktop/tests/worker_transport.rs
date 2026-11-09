//! Exercise the actual GUI-subsystem binary, without opening a window.
#![cfg(windows)]

use browser_ipc::{DocumentInput, Viewport};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn executable() -> &'static Path {
    static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();
    // Release verification may select the trusted, hash-identified delivery;
    // ordinary cargo test always uses the actual GUI binary built by Cargo.
    EXECUTABLE.get_or_init(|| {
        std::env::var_os("NOIR_TEST_DESKTOP_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_noir")))
    })
}

#[test]
fn desktop_binary_verifies_appcontainer_and_renders_remote_bytes() {
    browser_process::verify_remote_sandbox(executable())
        .expect("the installed GUI binary must return a complete isolation proof");
    let document = DocumentInput {
        url: "https://example.com/controlled-test".into(),
        html: "<title>Desktop sandbox</title><p>Authorized bytes, no worker network</p>".into(),
        audio_metadata: vec![],
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let page = browser_process::render_in_subprocess(executable(), document, Viewport::default())
        .expect("the GUI worker must use its inherited response pipe");
    assert_eq!(page.title, "Desktop sandbox");
    assert_ne!(page.metrics.worker_pid, std::process::id());
}

#[test]
fn desktop_local_and_sandboxed_sessions_can_start_concurrently() {
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for remote in [false, true] {
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..3 {
                    let document = DocumentInput {
                        url: if remote {
                            "https://example.com/controlled"
                        } else {
                            "noir:test"
                        }
                        .into(),
                        html: "<title>Concurrent desktop</title><p>Transport remains bounded</p>"
                            .into(),
                        audio_metadata: vec![],
                        images: vec![],
                        scripts: vec![],
                        stylesheets: vec![],
                    };
                    let (mut session, first) = browser_process::RendererSession::spawn(
                        executable(),
                        document,
                        Viewport::default(),
                    )
                    .expect("concurrent desktop worker must initialize");
                    assert_eq!(first.title, "Concurrent desktop");
                    let next = session
                        .render(
                            Viewport {
                                width: 640.,
                                height: 480.,
                            },
                            None,
                        )
                        .unwrap();
                    assert_eq!(next.title, "Concurrent desktop");
                    assert_eq!(next.metrics.worker_pid, first.metrics.worker_pid);
                }
            });
        }
    });
}
