//! A retained document's narrow capability for dynamic classic script sources.
//! The initial URL/root and attempt/byte budgets never come from worker replies.
use browser_ipc::{
    DynamicScriptRequest, DynamicScriptResponse, MAX_DYNAMIC_SCRIPT_BYTES,
    MAX_DYNAMIC_SCRIPT_TOTAL_BYTES, MAX_DYNAMIC_SCRIPTS, validate_dynamic_script_requests,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use url::Url;

const NETWORK_BUDGET: Duration = Duration::from_secs(25);

enum Authority {
    Denied,
    Local { base: Url, root: PathBuf },
    Remote(Url),
}

pub struct DynamicScriptBroker {
    authority: Authority,
    attempted: HashSet<u64>,
    source_bytes: usize,
    network_time: Duration,
}

impl DynamicScriptBroker {
    /// Called only by the privileged session host, before accepting worker data.
    /// Unsupported/synthetic documents have no subresource authority.
    pub fn new(document_url: &str, executable: &Path) -> Result<Self, String> {
        let authority = match Url::parse(document_url) {
            Ok(base) if matches!(base.scheme(), "https" | "http") => {
                super::network::validate_url(&base)?;
                security_sandbox::ensure_verified(executable)?;
                Authority::Remote(base)
            }
            Ok(base) if base.scheme() == "file" => {
                match super::local_path(document_url)
                    .and_then(|path| path.canonicalize().map_err(|e| e.to_string()))
                {
                    Ok(path) if path.is_file() => {
                        match (path.parent(), Url::from_file_path(&path)) {
                            (Some(root), Ok(base)) => Authority::Local {
                                base,
                                root: root.to_owned(),
                            },
                            _ => Authority::Denied,
                        }
                    }
                    _ => Authority::Denied,
                }
            }
            _ => Authority::Denied,
        };
        Ok(Self {
            authority,
            attempted: HashSet::new(),
            source_bytes: 0,
            network_time: Duration::ZERO,
        })
    }

    /// Validate the complete batch before starting any I/O or committing it.
    /// IDs remain spent after failure, DOM rollback or resource removal.
    pub fn validate_requests(&self, requests: &[DynamicScriptRequest]) -> Result<(), String> {
        validate_dynamic_script_requests(requests)?;
        if self.attempted.len().saturating_add(requests.len()) > MAX_DYNAMIC_SCRIPTS
            || requests
                .iter()
                .any(|request| self.attempted.contains(&request.id))
        {
            return Err("Dynamic script attempt quota or identity violated".into());
        }
        Ok(())
    }

    pub fn resolve(
        &mut self,
        requests: &[DynamicScriptRequest],
    ) -> Result<Vec<DynamicScriptResponse>, String> {
        self.validate_requests(requests)?;
        // Charge the entire validated batch before fetching. There is no retry
        // path which can renew the session's budgets.
        self.attempted
            .extend(requests.iter().map(|request| request.id));
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            let limit =
                (MAX_DYNAMIC_SCRIPT_TOTAL_BYTES - self.source_bytes).min(MAX_DYNAMIC_SCRIPT_BYTES);
            let source = if limit == 0 {
                None
            } else {
                match &self.authority {
                    Authority::Denied => None,
                    Authority::Local { base, root } => {
                        super::local_resource(&request.src, root, base, limit, "js")
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                            .map(|source| {
                                source
                                    .strip_prefix('\u{feff}')
                                    .unwrap_or(&source)
                                    .to_owned()
                            })
                    }
                    Authority::Remote(base) => {
                        let started = Instant::now();
                        let source = if self.network_time >= NETWORK_BUDGET {
                            None
                        } else {
                            super::network::load_dynamic_script(
                                base,
                                &request.src,
                                limit,
                                self.network_time,
                            )
                            .ok()
                        };
                        self.network_time = self.network_time.saturating_add(started.elapsed());
                        source
                    }
                }
            }
            .filter(|source| source.len() <= limit);
            self.source_bytes += source.as_ref().map_or(0, String::len);
            responses.push(DynamicScriptResponse {
                id: request.id,
                source,
            });
        }
        Ok(responses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn request(id: u64, src: &str) -> DynamicScriptRequest {
        DynamicScriptRequest {
            id,
            src: src.into(),
        }
    }
    fn local(root: &Path) -> DynamicScriptBroker {
        fs::write(root.join("index.html"), "<p>fixture</p>").unwrap();
        DynamicScriptBroker::new(
            Url::from_file_path(root.join("index.html"))
                .unwrap()
                .as_str(),
            &std::env::current_exe().unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn local_loads_are_confined_bounded_and_keep_initial_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("allowed");
        fs::create_dir(&root).unwrap();
        fs::write(dir.path().join("private.js"), "secret").unwrap();
        fs::write(root.join("ok.js"), "\u{feff}window.value = 1;").unwrap();
        fs::write(root.join("text.txt"), "secret").unwrap();
        let mut broker = local(&root);
        let result = broker
            .resolve(&[
                request(1, "ok.js"),
                request(2, "../private.js"),
                request(3, "text.txt"),
                request(4, "https://example.com/x.js"),
                request(5, "//server/private.js"),
                request(6, "file:///tmp/private.js"),
            ])
            .unwrap();
        assert_eq!(result[0].source.as_deref(), Some("window.value = 1;"));
        assert!(result[1..].iter().all(|response| response.source.is_none()));
        assert!(broker.resolve(&[request(1, "ok.js")]).is_err());
        fs::write(
            root.join("large.js"),
            "x".repeat(MAX_DYNAMIC_SCRIPT_BYTES + 1),
        )
        .unwrap();
        assert!(
            broker.resolve(&[request(7, "large.js")]).unwrap()[0]
                .source
                .is_none()
        );
    }
    #[test]
    fn failed_attempts_and_source_bytes_are_cumulative() {
        let dir = tempfile::tempdir().unwrap();
        let mut broker = local(dir.path());
        fs::write(
            dir.path().join("full.js"),
            "x".repeat(MAX_DYNAMIC_SCRIPT_BYTES),
        )
        .unwrap();
        for id in 1..=4 {
            assert_eq!(
                broker.resolve(&[request(id, "full.js")]).unwrap()[0]
                    .source
                    .as_ref()
                    .unwrap()
                    .len(),
                MAX_DYNAMIC_SCRIPT_BYTES
            );
        }
        assert!(
            broker.resolve(&[request(5, "full.js")]).unwrap()[0]
                .source
                .is_none()
        );
        for id in 6..=MAX_DYNAMIC_SCRIPTS as u64 {
            assert!(
                broker.resolve(&[request(id, "missing.js")]).unwrap()[0]
                    .source
                    .is_none()
            );
        }
        assert!(broker.resolve(&[request(33, "full.js")]).is_err());
        assert_eq!(broker.source_bytes, MAX_DYNAMIC_SCRIPT_TOTAL_BYTES);
        assert_eq!(broker.attempted.len(), MAX_DYNAMIC_SCRIPTS);
    }
    #[test]
    fn invalid_batch_does_not_spend_ids_or_read_earlier_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut broker = local(dir.path());
        fs::write(dir.path().join("ok.js"), "value = 1;").unwrap();
        for requests in [
            vec![request(1, "ok.js"), request(1, "duplicate.js")],
            vec![request(1, "ok.js"), request(2, "bad\n.js")],
            vec![request(1, "ok.js"); MAX_DYNAMIC_SCRIPTS + 1],
        ] {
            assert!(broker.resolve(&requests).is_err());
            assert!(broker.attempted.is_empty());
            assert_eq!(broker.source_bytes, 0);
        }
        assert!(
            broker.resolve(&[request(1, "ok.js")]).unwrap()[0]
                .source
                .is_some()
        );
    }
    #[test]
    fn internal_pages_never_gain_network_or_file_authority() {
        let mut broker =
            DynamicScriptBroker::new("noir:welcome", Path::new("nonexistent-executable")).unwrap();
        let responses = broker
            .resolve(&[
                request(1, "https://example.com/script.js"),
                request(2, "file:///tmp/script.js"),
                request(3, "script.js"),
            ])
            .unwrap();
        assert!(responses.iter().all(|response| response.source.is_none()));
    }
    #[cfg(not(windows))]
    #[test]
    fn remote_broker_fails_closed_without_verified_sandbox() {
        assert!(
            DynamicScriptBroker::new(
                "https://example.com/index.html",
                &std::env::current_exe().unwrap()
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn local_symlinks_cannot_escape_original_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("allowed");
        fs::create_dir(&root).unwrap();
        fs::write(dir.path().join("private.js"), "secret").unwrap();
        std::os::unix::fs::symlink(dir.path().join("private.js"), root.join("escape.js")).unwrap();
        let mut broker = local(&root);
        assert!(
            broker.resolve(&[request(1, "escape.js")]).unwrap()[0]
                .source
                .is_none()
        );
    }
}
