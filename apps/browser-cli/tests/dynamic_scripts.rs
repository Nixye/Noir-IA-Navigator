//! Real retained-worker and privileged-broker integration, using local fixtures.
use browser_ipc::{DisplayItem, DocumentInput, RenderedPage, Viewport};
use std::{fs, path::Path};

fn executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_browser-cli"))
}
fn text(page: &RenderedPage) -> String {
    page.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn document(root: &Path, html: &str) -> DocumentInput {
    let file = root.join("index.html");
    fs::write(&file, html).unwrap();
    browser_core::load_document(file.to_str().unwrap()).unwrap()
}

#[test]
fn real_session_brokers_chained_sources_and_keeps_callbacks_and_vm_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("first.js"),
        r#"
        count++;
        document.getElementById('state').textContent = 'first-' + count;
        var second = document.createElement('script');
        second.src = 'second.js';
        second.onload = () => { document.getElementById('state').textContent += '-second-load'; };
        document.head.appendChild(second);
    "#,
    )
    .unwrap();
    fs::write(
        dir.path().join("second.js"),
        r#"
        count++;
        document.getElementById('state').textContent += '-second-' + count;
    "#,
    )
    .unwrap();
    fs::write(
        dir.path().join("timer.js"),
        r#"
        count++;
        document.getElementById('state').textContent = 'timer-' + count;
    "#,
    )
    .unwrap();
    let input = document(
        dir.path(),
        r#"<p id=state>waiting</p><script>
        var count = 0;
        var first = document.createElement('script');
        first.src = 'first.js';
        first.onload = () => { document.getElementById('state').textContent += '-first-load'; };
        document.head.appendChild(first);
        setTimeout(() => {
            var late = document.createElement('script');
            late.src = 'timer.js';
            late.onload = () => { document.getElementById('state').textContent += '-timer-load'; };
            document.head.appendChild(late);
        }, 10);
    </script>"#,
    );
    // Discovery does not prefetch script names contained inside JavaScript.
    assert!(input.scripts.is_empty());
    let (mut session, first) =
        browser_process::RendererSession::spawn(executable(), input, Viewport::default()).unwrap();
    assert_eq!(
        text(&first),
        "first-1-first-load-second-2-second-load",
        "warnings: {:?}",
        first.warnings
    );
    let pid = first.metrics.worker_pid;
    let reflow = session
        .render(
            Viewport {
                width: 640.,
                height: 480.,
            },
            None,
        )
        .unwrap();
    assert_eq!(text(&reflow), text(&first));
    assert_eq!(reflow.metrics.worker_pid, pid);
    let later = session.advance_timers(Viewport::default(), 10).unwrap();
    assert_eq!(
        text(&later),
        "timer-3-timer-load",
        "warnings: {:?}",
        later.warnings
    );
    assert_eq!(later.metrics.worker_pid, pid);
    let unchanged = session.advance_timers(Viewport::default(), 100).unwrap();
    assert_eq!(text(&unchanged), text(&later));
}

#[test]
fn real_session_denials_dispatch_errors_without_reading_outside_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("allowed");
    fs::create_dir(&root).unwrap();
    fs::write(
        dir.path().join("private.js"),
        "document.getElementById('state').textContent = 'SECRET';",
    )
    .unwrap();
    let input = document(
        &root,
        r#"<p id=state>safe</p><script>
        var blocked = document.createElement('script');
        blocked.src = '../private.js';
        blocked.onload = () => { document.getElementById('state').textContent = 'unexpected-load'; };
        blocked.onerror = () => { document.getElementById('state').textContent += '-denied'; };
        document.head.appendChild(blocked);
        var absent = document.createElement('script');
        absent.src = 'missing.js';
        absent.onerror = () => { document.getElementById('state').textContent += '-missing'; };
        document.head.appendChild(absent);
    </script>"#,
    );
    let (_session, page) =
        browser_process::RendererSession::spawn(executable(), input, Viewport::default()).unwrap();
    assert_eq!(
        text(&page),
        "safe-denied-missing",
        "warnings: {:?}",
        page.warnings
    );
}

#[test]
fn one_shot_rendering_preserves_offline_boundary_and_reports_pending_scripts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("external.js"),
        "document.getElementById('state').textContent = 'loaded';",
    )
    .unwrap();
    let input = document(
        dir.path(),
        r#"<p id=state>offline</p><script>
        var source = document.createElement('script');
        source.src = 'external.js';
        document.head.appendChild(source);
    </script>"#,
    );
    let page =
        browser_process::render_in_subprocess(executable(), input, Viewport::default()).unwrap();
    assert_eq!(text(&page), "offline");
    assert!(
        page.warnings
            .iter()
            .any(|warning| warning.contains("retained renderer session")),
        "{:?}",
        page.warnings
    );
}

#[test]
fn internal_document_dynamic_source_gets_an_error_and_no_file_authority() {
    let input = DocumentInput {
        url: "noir:dynamic-test".into(),
        html: r#"<p id=state>safe</p><script>
            var source = document.createElement('script');
            source.src = 'https://example.com/external.js';
            source.onerror = () => { document.getElementById('state').textContent += '-denied'; };
            document.head.appendChild(source);
        </script>"#
            .into(),
        images: vec![],
        audio_metadata: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let (_session, page) =
        browser_process::RendererSession::spawn(executable(), input, Viewport::default()).unwrap();
    assert_eq!(text(&page), "safe-denied", "warnings: {:?}", page.warnings);
}

#[test]
fn real_worker_rejects_unsolicited_completions_before_evaluation() {
    use browser_ipc::{
        DynamicScriptResponse, PROTOCOL_VERSION, RenderRequest, RenderResponse, SessionRequest,
        read_message, write_message,
    };
    use std::process::{Command, Stdio};
    let input = DocumentInput {
        url: "noir:test".into(),
        html: "<p>safe</p>".into(),
        images: vec![],
        audio_metadata: vec![],
        scripts: vec![],
        stylesheets: vec![],
    };
    let mut child = Command::new(executable())
        .arg("--renderer-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    write_message(
        &mut stdin,
        &SessionRequest::Begin(RenderRequest {
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            document: input,
            viewport: Viewport::default(),
            session_storage: Default::default(),
        }),
    )
    .unwrap();
    let response: RenderResponse = read_message(&mut stdout).unwrap();
    assert!(response.result.is_ok());
    assert!(response.pending_scripts.is_empty());
    write_message(
        &mut stdin,
        &SessionRequest::Scripts {
            id: 2,
            scripts: vec![DynamicScriptResponse {
                id: 1,
                source: Some("document.title = 'unexpected';".into()),
            }],
            viewport: Viewport::default(),
        },
    )
    .unwrap();
    let response: RenderResponse = read_message(&mut stdout).unwrap();
    assert!(response.result.unwrap_err().contains("identity"));
    assert!(response.pending_scripts.is_empty());
    drop(stdin);
    assert!(child.wait().unwrap().success());
}
