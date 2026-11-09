use browser_cli::snapshot::{self, Snapshot};
use browser_ipc::{DocumentInput, RenderedPage, ScriptAsset, Viewport};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_browser-cli"));
    command.args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output().unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn local_fixture(directory: &Path) -> std::path::PathBuf {
    for (name, bytes) in [
        (
            "snapshot-replay.html",
            include_str!("fixtures/snapshot-replay.html"),
        ),
        (
            "snapshot-replay.js",
            include_str!("fixtures/snapshot-replay.js"),
        ),
        (
            "snapshot-replay.css",
            include_str!("fixtures/snapshot-replay.css"),
        ),
    ] {
        fs::write(directory.join(name), bytes).unwrap();
    }
    directory.join("snapshot-replay.html")
}

#[test]
fn capture_then_replay_uses_preserved_bytes_after_all_original_files_are_removed() {
    let directory = tempfile::tempdir().unwrap();
    let html = local_fixture(directory.path());
    let expected = browser_core::load_document(path(&html)).unwrap();
    let snapshot_path = directory.path().join("capture.noir-snapshot");
    let capture = cli(&[
        "capture",
        path(&html),
        "--snapshot",
        path(&snapshot_path),
        "--width",
        "743",
        "--height",
        "481",
    ]);
    assert_success(&capture);
    assert!(capture.stdout.is_empty(), "capture must not render");
    let captured = snapshot::load(&snapshot_path).unwrap();
    assert_eq!(
        serde_json::to_value(&captured.document).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(captured.viewport.width, 743.);
    assert_eq!(captured.viewport.height, 481.);
    assert!(captured.document.html.contains("Before replay"));
    for name in [
        "snapshot-replay.html",
        "snapshot-replay.js",
        "snapshot-replay.css",
    ] {
        fs::remove_file(directory.path().join(name)).unwrap();
    }
    let output_path = directory.path().join("replayed.json");
    let ppm_path = directory.path().join("replayed.ppm");
    let replay = cli(&[
        "replay",
        path(&snapshot_path),
        "--output",
        path(&output_path),
        "--ppm",
        path(&ppm_path),
    ]);
    assert_success(&replay);
    let page: RenderedPage = serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
    assert_eq!(page.title, "Snapshot replay");
    assert_eq!(page.width, 743.);
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    assert!(page.items.iter().any(|item| matches!(item, browser_ipc::DisplayItem::Text { text, color, .. } if text == "Preserved" && *color == browser_ipc::Color(18, 52, 86, 255))));
    assert!(page.metrics.worker_pid > 0);
    assert_ne!(page.metrics.worker_pid, std::process::id());
    assert!(
        fs::read(ppm_path)
            .unwrap()
            .starts_with(b"P6\n743 481\n255\n")
    );
}

#[test]
fn snapshot_and_replay_output_writes_never_overwrite_existing_files() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot_path = directory.path().join("capture.noir-snapshot");
    let capture = cli(&[
        "capture",
        "noir:welcome",
        "--snapshot",
        path(&snapshot_path),
    ]);
    assert_success(&capture);
    let original = fs::read(&snapshot_path).unwrap();
    // Missing input demonstrates the existing destination is refused before broker I/O.
    let rejected = cli(&[
        "capture",
        "missing-fixture.html",
        "--snapshot",
        path(&snapshot_path),
    ]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("já existe"));
    assert_eq!(fs::read(&snapshot_path).unwrap(), original);
    let captured = snapshot::load(&snapshot_path).unwrap();
    assert!(snapshot::save_new(&snapshot_path, &captured).is_err());
    assert_eq!(fs::read(&snapshot_path).unwrap(), original);
    for output_flag in ["--output", "--ppm"] {
        let rejected = cli(&[
            "replay",
            path(&snapshot_path),
            output_flag,
            path(&snapshot_path),
        ]);
        assert!(!rejected.status.success());
        assert_eq!(fs::read(&snapshot_path).unwrap(), original);
    }
}

#[test]
fn diagnostics_are_opt_in_and_replay_cannot_override_viewport_or_recapture() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot_path = directory.path().join("not-created.noir-snapshot");
    for arguments in [
        vec!["render", "noir:welcome", "--snapshot", path(&snapshot_path)],
        vec!["capture", "noir:welcome"],
        vec![
            "capture",
            "noir:welcome",
            "--snapshot",
            path(&snapshot_path),
            "--output",
            "unused.json",
        ],
        vec!["replay", path(&snapshot_path), "--width", "900"],
        vec![
            "replay",
            path(&snapshot_path),
            "--snapshot",
            path(&snapshot_path),
        ],
    ] {
        assert!(!cli(&arguments).status.success());
        assert!(!snapshot_path.exists());
    }
    let rendered = cli(&["render", "noir:welcome"]);
    assert_success(&rendered);
    assert!(!snapshot_path.exists());
}

#[test]
fn malformed_and_oversized_files_fail_before_render_or_output() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot_path = directory.path().join("bad.noir-snapshot");
    let output_path = directory.path().join("must-not-exist.json");
    fs::write(&snapshot_path, b"NOIRSNAP").unwrap();
    assert!(
        !cli(&[
            "replay",
            path(&snapshot_path),
            "--output",
            path(&output_path)
        ])
        .status
        .success()
    );
    assert!(!output_path.exists());
    fs::File::create(&snapshot_path)
        .unwrap()
        .set_len(snapshot::MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    assert!(
        snapshot::load(&snapshot_path)
            .unwrap_err()
            .contains("12 MiB")
    );
    assert!(
        !cli(&[
            "replay",
            path(&snapshot_path),
            "--output",
            path(&output_path)
        ])
        .status
        .success()
    );
    assert!(!output_path.exists());
}

fn remote_fixture() -> Snapshot {
    Snapshot {
        document: DocumentInput {
            url: "https://snapshot-replay.invalid/final?preserve=%2F#origin".into(),
            html: "<title>Before</title><p id=status>Before</p><script src=preserved.js></script>".into(),
            images: vec![], audio_metadata: vec![], stylesheets: vec![],
            scripts: vec![ScriptAsset {
                src: "preserved.js".into(),
                source: "document.title=location.href;document.getElementById('status').textContent='UnexpectedStorage';try { window.sessionStorage.getItem('private'); } catch (error) { document.getElementById('status').textContent='Preserved'; }".into(),
            }],
        },
        viewport: Viewport { width: 640., height: 480. },
    }
}

#[cfg(windows)]
#[test]
fn remote_snapshot_replay_preserves_https_origin_inside_the_normal_appcontainer() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot_path = directory.path().join("remote.noir-snapshot");
    let snapshot = remote_fixture();
    let expected_url = snapshot.document.url.clone();
    snapshot::save_new(&snapshot_path, &snapshot).unwrap();
    let replay = cli(&["replay", path(&snapshot_path)]);
    assert_success(&replay);
    let page: RenderedPage = serde_json::from_slice(&replay.stdout).unwrap();
    // .invalid is never fetched. location.href comes from the preserved input,
    // which render_in_subprocess must route through verified AppContainer.
    assert_eq!(page.title, expected_url);
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    assert_eq!(page.width, 640.);
    assert!(page.items.iter().any(
        |item| matches!(item, browser_ipc::DisplayItem::Text { text, .. } if text == "Preserved")
    ));
}

#[cfg(not(windows))]
#[test]
fn remote_snapshot_replay_fails_closed_without_supported_os_sandbox() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot_path = directory.path().join("remote.noir-snapshot");
    snapshot::save_new(&snapshot_path, &remote_fixture()).unwrap();
    let replay = cli(&["replay", path(&snapshot_path)]);
    assert!(!replay.status.success());
    assert!(replay.stdout.is_empty());
}
