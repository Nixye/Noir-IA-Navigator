//! Fail-closed OS sandbox gate for remote documents. No web engine code lives here.
use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use std::io::{Read, Write};
use std::{path::Path, sync::OnceLock};
#[cfg(windows)]
use std::{sync::mpsc, time::Duration};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{SandboxedChild, is_appcontainer, spawn};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub appcontainer: bool,
    pub zero_capabilities: bool,
    pub private_file_denied: bool,
    pub private_write_denied: bool,
    pub internet_denied: bool,
    pub child_process_denied: bool,
}

/// Serialize known host-side process launches with the short Windows handle
/// inheritance window used by the AppContainer launcher. This does not wait for
/// the child or grant it additional access.
pub fn spawn_host_process(
    command: &mut std::process::Command,
) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    let _guard = windows::spawn_guard().map_err(std::io::Error::other)?;
    command.spawn()
}

#[cfg(any(windows, test))]
const PROOF_MAGIC: [u8; 4] = *b"NSP1";
#[cfg(any(windows, test))]
const MAX_PROOF_BYTES: usize = 4096;

#[cfg(any(windows, test))]
fn write_proof(writer: &mut impl Write, proof: &Proof) -> Result<(), String> {
    let bytes = serde_json::to_vec(proof).map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_PROOF_BYTES {
        return Err("Sandbox proof size is invalid".into());
    }
    writer.write_all(&PROOF_MAGIC).map_err(|e| e.to_string())?;
    writer
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&bytes).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

#[cfg(any(windows, test))]
fn read_proof(reader: &mut impl Read) -> Result<Proof, String> {
    let mut header = [0; 8];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("Sandbox proof header: {e}"))?;
    if header[..4] != PROOF_MAGIC {
        return Err("Sandbox proof protocol mismatch".into());
    }
    let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if length == 0 || length > MAX_PROOF_BYTES {
        return Err("Sandbox proof size is invalid".into());
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| format!("Sandbox proof payload: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid sandbox proof: {e}"))
}

impl Proof {
    pub fn verified(&self) -> bool {
        self.appcontainer
            && self.zero_capabilities
            && self.private_file_denied
            && self.private_write_denied
            && self.internet_denied
            && self.child_process_denied
    }
}

/// Runs before any page-controlled network request. A failed probe is cached as
/// a denial, never converted into a weaker renderer launch.
pub fn ensure_verified(executable: &Path) -> Result<(), String> {
    static VERIFIED: OnceLock<Result<(), String>> = OnceLock::new();
    VERIFIED.get_or_init(|| verify(executable)).clone()
}

#[cfg(windows)]
fn verify(executable: &Path) -> Result<(), String> {
    let dir = tempfile::Builder::new()
        .prefix("noir-sandbox-proof-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let private = dir.path().join("private-canary.txt");
    std::fs::write(&private, b"Noir sandbox canary: never page data").map_err(|e| e.to_string())?;
    let mut child = spawn(
        executable,
        "--sandbox-probe",
        Some(private.to_string_lossy().as_ref()),
    )?;
    drop(child.stdin.take());
    let mut stdout = child.stdout.take().ok_or("Sandbox stdout unavailable")?;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        // A sibling host process must not hold up a complete proof just because
        // it retained a pipe writer. A frame, never EOF, delimits the response.
        let result = read_proof(&mut stdout);
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "A verificação AppContainer excedeu o prazo".to_string())
        .and_then(|r| r);
    let exit = child.exit_code();
    child.terminate();
    let proof = result.map_err(|e| format!("{e} (sandbox exit {exit:#x})"))?;
    if !proof.verified() {
        return Err(format!("Isolamento AppContainer não confirmado: {proof:?}"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify(_executable: &Path) -> Result<(), String> {
    Err("Navegação remota indisponível: sandbox verificável ainda não implementada nesta plataforma".into())
}

#[cfg(not(windows))]
pub fn is_appcontainer() -> bool {
    false
}

/// Trusted executable entry point; the argument is a deliberately inaccessible
/// host canary path. Only the proof booleans are emitted, never file contents.
pub fn run_probe(private_path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let proof = windows::probe(private_path)?;
        write_proof(&mut std::io::stdout().lock(), &proof)
    }
    #[cfg(not(windows))]
    {
        let _ = private_path;
        Err("Sandbox probe unsupported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid_proof() -> Proof {
        Proof {
            appcontainer: true,
            zero_capabilities: true,
            private_file_denied: true,
            private_write_denied: true,
            internet_denied: true,
            child_process_denied: true,
        }
    }
    #[test]
    fn proof_frame_is_complete_without_reading_to_eof() {
        struct NoEof(std::io::Cursor<Vec<u8>>);
        impl Read for NoEof {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                assert!(
                    self.0.position() < self.0.get_ref().len() as u64,
                    "the proof reader must not ask for EOF after the complete frame"
                );
                self.0.read(bytes)
            }
        }
        let mut bytes = Vec::new();
        write_proof(&mut bytes, &valid_proof()).unwrap();
        let mut reader = NoEof(std::io::Cursor::new(bytes));
        assert!(read_proof(&mut reader).unwrap().verified());
    }
    #[test]
    fn proof_frames_reject_invalid_headers_lengths_and_fields() {
        let mut valid = Vec::new();
        write_proof(&mut valid, &valid_proof()).unwrap();
        for end in [0, 3, 7, valid.len() - 1] {
            assert!(read_proof(&mut &valid[..end]).is_err());
        }
        let mut wrong_magic = valid.clone();
        wrong_magic[0] = b'X';
        assert!(read_proof(&mut wrong_magic.as_slice()).is_err());
        for length in [0, 4097, u32::MAX] {
            let mut header = PROOF_MAGIC.to_vec();
            header.extend(length.to_le_bytes());
            assert!(read_proof(&mut header.as_slice()).is_err());
        }
        for payload in [
            br#"{}"#.as_slice(),
            br#"{"appcontainer":true,"zero_capabilities":true,"private_file_denied":true,"private_write_denied":true,"internet_denied":true,"child_process_denied":true,"unknown":true}"#,
            br#"{"appcontainer":"true"}"#,
        ] {
            let mut bytes = PROOF_MAGIC.to_vec();
            bytes.extend((payload.len() as u32).to_le_bytes());
            bytes.extend(payload);
            assert!(read_proof(&mut bytes.as_slice()).is_err());
        }
        let mut denied = valid_proof();
        denied.internet_denied = false;
        let mut bytes = Vec::new();
        write_proof(&mut bytes, &denied).unwrap();
        assert!(!read_proof(&mut bytes.as_slice()).unwrap().verified());
    }
    #[test]
    fn every_os_proof_is_required_to_enable_remote_content() {
        let mut proof = Proof {
            appcontainer: true,
            zero_capabilities: true,
            private_file_denied: true,
            private_write_denied: true,
            internet_denied: true,
            child_process_denied: true,
        };
        assert!(proof.verified());
        for index in 0..6 {
            let fields = [
                &mut proof.appcontainer,
                &mut proof.zero_capabilities,
                &mut proof.private_file_denied,
                &mut proof.private_write_denied,
                &mut proof.internet_denied,
                &mut proof.child_process_denied,
            ];
            *fields[index] = false;
            assert!(!proof.verified());
            let fields = [
                &mut proof.appcontainer,
                &mut proof.zero_capabilities,
                &mut proof.private_file_denied,
                &mut proof.private_write_denied,
                &mut proof.internet_denied,
                &mut proof.child_process_denied,
            ];
            *fields[index] = true;
        }
    }
}
