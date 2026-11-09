//! Win32 calls and owning handle wrappers. Pointers never escape this module.
use crate::Proof;
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Isolation::*, *},
    Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ},
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

static ID: AtomicU64 = AtomicU64::new(1);
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn spawn_guard() -> Result<MutexGuard<'static, ()>, String> {
    SPAWN_LOCK
        .lock()
        .map_err(|_| "Host process launch coordination is unavailable".into())
}
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn error(context: &str) -> String {
    format!("{context}: {}", io::Error::last_os_error())
}
fn checked(result: i32, context: &str) -> Result<(), String> {
    if result == 0 {
        Err(error(context))
    } else {
        Ok(())
    }
}
fn handle(value: HANDLE) -> Result<OwnedHandle, String> {
    if value.is_null() || value == INVALID_HANDLE_VALUE {
        return Err(error("Invalid sandbox handle"));
    }
    // SAFETY: this is a newly created, uniquely owned Win32 kernel handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(value) })
}
fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}

struct Profile {
    name: Vec<u16>,
    // Owning address stored as an integer so the uniquely owned profile can
    // move with a renderer session between task threads. It is never shared.
    sid: usize,
}
impl Profile {
    fn create() -> Result<Self, String> {
        let name = wide(format!(
            "Noir.Renderer.{}.{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut sid = null_mut();
        // SAFETY: null-terminated strings remain alive; output SID is freed in Drop.
        let status = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if status < 0 {
            return Err(format!("CreateAppContainerProfile HRESULT {status:#x}"));
        }
        if sid.is_null() {
            return Err("AppContainer returned an empty SID".into());
        }
        Ok(Self {
            name,
            sid: sid as usize,
        })
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        // SAFETY: profile name/SID were returned by successful profile creation.
        unsafe {
            DeleteAppContainerProfile(self.name.as_ptr());
            FreeSid(self.sid as PSID);
        }
    }
}

fn grant_runtime_read(path: &Path, sid: PSID) -> Result<(), String> {
    let path = wide(path);
    let mut old_acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: output pointers are valid; allocated descriptors are LocalFree'd below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(format!("Read runtime ACL: {status}"));
    }
    let mut access: EXPLICIT_ACCESS_W = unsafe { zeroed() };
    access.grfAccessPermissions = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;
    access.grfAccessMode = GRANT_ACCESS;
    access.grfInheritance = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
    access.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    access.Trustee.TrusteeType = TRUSTEE_IS_GROUP;
    access.Trustee.ptstrName = sid.cast();
    let mut new_acl = null_mut();
    // SAFETY: all ACL and SID pointers refer to live Win32 allocations.
    let status = unsafe { SetEntriesInAclW(1, &access, old_acl, &mut new_acl) };
    let result = if status != 0 {
        Err(format!("Build runtime ACL: {status}"))
    } else {
        let status = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                new_acl,
                null(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Apply runtime ACL: {status}"))
        }
    };
    unsafe {
        if !new_acl.is_null() {
            LocalFree(new_acl.cast());
        }
        if !descriptor.is_null() {
            LocalFree(descriptor);
        }
    }
    result
}

/// An AppContainer worker with explicit pipes and a kill-on-close Job. The
/// runtime directory and AppContainer profile outlive the process, then expire.
pub struct SandboxedChild {
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    process: OwnedHandle,
    _job: OwnedHandle,
    _runtime: tempfile::TempDir,
    _profile: Profile,
}
impl SandboxedChild {
    pub fn exit_code(&self) -> u32 {
        let mut code = 0;
        unsafe {
            GetExitCodeProcess(raw(&self.process), &mut code);
        }
        code
    }
    pub fn terminate(&mut self) {
        unsafe {
            TerminateProcess(raw(&self.process), 1);
            WaitForSingleObject(raw(&self.process), 5000);
        }
    }
}
impl Drop for SandboxedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

struct AttributeList {
    _storage: Vec<usize>,
    ptr: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl AttributeList {
    fn new(count: u32) -> Result<Self, String> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
        }
        if bytes == 0 || bytes > 64 * 1024 {
            return Err(error("Invalid sandbox attribute allocation"));
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let ptr = storage.as_mut_ptr().cast();
        checked(
            unsafe { InitializeProcThreadAttributeList(ptr, count, 0, &mut bytes) },
            "Initialize sandbox attributes",
        )?;
        Ok(Self {
            _storage: storage,
            ptr,
        })
    }
    fn set<T>(&mut self, attribute: u32, value: &T) -> Result<(), String> {
        checked(
            unsafe {
                UpdateProcThreadAttribute(
                    self.ptr,
                    0,
                    attribute as usize,
                    (value as *const T).cast(),
                    size_of::<T>(),
                    null_mut(),
                    null(),
                )
            },
            "Set sandbox attribute",
        )
    }
    fn set_handles(&mut self, handles: &[HANDLE]) -> Result<(), String> {
        checked(
            unsafe {
                UpdateProcThreadAttribute(
                    self.ptr,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.as_ptr().cast(),
                    size_of_val(handles),
                    null_mut(),
                    null(),
                )
            },
            "Set inherited handle allowlist",
        )
    }
}
impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.ptr);
        }
    }
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle), String> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 0,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    checked(
        unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
        "Create sandbox pipe",
    )?;
    Ok((handle(read)?, handle(write)?))
}

pub fn spawn(
    executable: &Path,
    mode: &str,
    argument: Option<&str>,
) -> Result<SandboxedChild, String> {
    if mode != "--renderer" && mode != "--renderer-session" && mode != "--sandbox-probe" {
        return Err("Invalid sandbox launch mode".into());
    }
    let profile = Profile::create()?;
    let runtime = tempfile::Builder::new()
        .prefix("noir-renderer-runtime-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    grant_runtime_read(runtime.path(), profile.sid as PSID)?;
    let copy = runtime.path().join("noir-renderer.exe");
    std::fs::copy(executable, &copy).map_err(|e| format!("Stage isolated renderer: {e}"))?;
    grant_runtime_read(&copy, profile.sid as PSID)?;
    let application = wide(&copy);
    let quote = |s: &str| format!("\"{}\"", s.replace('"', ""));
    let command = if let Some(argument) = argument {
        format!(
            "{} {} {}",
            quote(&copy.to_string_lossy()),
            mode,
            quote(argument)
        )
    } else {
        format!("{} {mode}", quote(&copy.to_string_lossy()))
    };
    let mut command = wide(command);
    // Coordinate with the AI/runtime and ordinary worker launchers. Only the
    // two child pipe ends become inheritable, during CreateProcessW itself.
    let _spawn_guard = spawn_guard()?;
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    checked(
        unsafe { SetHandleInformation(raw(&input_write), HANDLE_FLAG_INHERIT, 0) },
        "Restrict host write pipe",
    )?;
    checked(
        unsafe { SetHandleInformation(raw(&output_read), HANDLE_FLAG_INHERIT, 0) },
        "Restrict host read pipe",
    )?;
    let handles = [raw(&input_read), raw(&output_write)];
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid as PSID,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let child_policy = 1u32; // PROCESS_CREATION_CHILD_PROCESS_RESTRICTED
    let mut attrs = AttributeList::new(3)?;
    attrs.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
    attrs.set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &child_policy)?;
    attrs.set_handles(&handles)?;
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = raw(&input_read);
    startup.StartupInfo.hStdOutput = raw(&output_write);
    startup.StartupInfo.hStdError = raw(&output_write);
    startup.lpAttributeList = attrs.ptr;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    // Windows resolves AppContainer profile storage through LOCALAPPDATA even
    // for an unpackaged executable. This is an allowlist, never inherited secrets.
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA required for AppContainer")?;
    let mut environment = wide(format!("LOCALAPPDATA={}", local.to_string_lossy()));
    environment.extend(wide(format!("SystemRoot={}", root.to_string_lossy())));
    environment.push(0);
    let current_dir = wide(runtime.path());
    let mut information: PROCESS_INFORMATION = unsafe { zeroed() };
    for child_handle in handles {
        checked(
            unsafe { SetHandleInformation(child_handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
            "Allow intended child pipe inheritance",
        )?;
    }
    checked(
        unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                DETACHED_PROCESS
                    | CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                current_dir.as_ptr(),
                &startup.StartupInfo,
                &mut information,
            )
        },
        "Launch AppContainer",
    )?;
    let process = handle(information.hProcess)?;
    let thread = handle(information.hThread)?;
    let setup = (|| {
        for child_handle in handles {
            checked(
                unsafe { SetHandleInformation(child_handle, HANDLE_FLAG_INHERIT, 0) },
                "Close child pipe inheritance window",
            )?;
        }
        let job = handle(unsafe { CreateJobObjectW(null(), null()) })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 512 * 1024 * 1024;
        checked(
            unsafe {
                SetInformationJobObject(
                    raw(&job),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of_val(&limits) as u32,
                )
            },
            "Set renderer Job limits",
        )?;
        checked(
            unsafe { AssignProcessToJobObject(raw(&job), raw(&process)) },
            "Assign renderer Job",
        )?;
        if unsafe { ResumeThread(raw(&thread)) } == u32::MAX {
            return Err(error("Resume AppContainer"));
        }
        Ok(job)
    })();
    let job = match setup {
        Ok(job) => job,
        Err(e) => {
            unsafe {
                TerminateProcess(raw(&process), 1);
                WaitForSingleObject(raw(&process), 5000);
            }
            return Err(e);
        }
    };
    drop(input_read);
    drop(output_write);
    drop(thread);
    Ok(SandboxedChild {
        stdin: Some(File::from(input_write)),
        stdout: Some(File::from(output_read)),
        process,
        _job: job,
        _runtime: runtime,
        _profile: profile,
    })
}

fn token_value(class: TOKEN_INFORMATION_CLASS) -> Result<u32, String> {
    let mut token = null_mut();
    checked(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        "Open sandbox token",
    )?;
    let token = handle(token)?;
    let (mut value, mut needed) = (0u32, 0u32);
    checked(
        unsafe {
            GetTokenInformation(
                raw(&token),
                class,
                (&mut value as *mut u32).cast(),
                4,
                &mut needed,
            )
        },
        "Read sandbox token",
    )?;
    Ok(value)
}
pub fn is_appcontainer() -> bool {
    token_value(TokenIsAppContainer).is_ok_and(|v| v == 1)
}

fn zero_capabilities() -> Result<bool, String> {
    let mut token = null_mut();
    checked(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        "Open capability token",
    )?;
    let token = handle(token)?;
    let mut needed = 0;
    unsafe {
        GetTokenInformation(raw(&token), TokenCapabilities, null_mut(), 0, &mut needed);
    }
    if !(4..=64 * 1024).contains(&needed) {
        return Err(error("Invalid capability token allocation"));
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    checked(
        unsafe {
            GetTokenInformation(
                raw(&token),
                TokenCapabilities,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        },
        "Read sandbox capabilities",
    )?;
    Ok(unsafe { *(buffer.as_ptr().cast::<u32>()) } == 0)
}

pub fn probe(private_path: &str) -> Result<Proof, String> {
    let denied = |result: io::Result<File>| matches!(result,Err(error) if error.kind()==io::ErrorKind::PermissionDenied);
    let private_file_denied = denied(File::open(private_path));
    let private_write_denied = denied(File::create(
        PathBuf::from(private_path).with_extension("write-canary"),
    ));
    let network = std::net::TcpStream::connect_timeout(
        &"1.1.1.1:443"
            .parse()
            .map_err(|_| "Invalid proof endpoint")?,
        Duration::from_secs(2),
    );
    let internet_denied = matches!(network,Err(error) if error.kind()==io::ErrorKind::PermissionDenied || error.raw_os_error()==Some(10013));
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let child = std::process::Command::new(executable)
        .arg("--sandbox-probe-child")
        .spawn();
    let child_process_denied = match child {
        Err(e) => {
            e.kind() == io::ErrorKind::PermissionDenied
                || e.raw_os_error() == Some(ERROR_CHILD_PROCESS_BLOCKED as i32)
        }
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            false
        }
    };
    Ok(Proof {
        appcontainer: is_appcontainer(),
        zero_capabilities: zero_capabilities()?,
        private_file_denied,
        private_write_denied,
        internet_denied,
        child_process_denied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn complete_proof_returns_while_another_pipe_writer_stays_open() {
        let (read, write) = pipe().unwrap();
        let mut writer = File::from(write);
        let proof = Proof {
            appcontainer: true,
            zero_capabilities: true,
            private_file_denied: true,
            private_write_denied: true,
            internet_denied: true,
            child_process_denied: true,
        };
        crate::write_proof(&mut writer, &proof).unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let result = crate::read_proof(&mut File::from(read));
            let _ = sender.send(result);
        });
        // read_to_end cannot finish here: the parent deliberately retains the
        // writer, reproducing the EOF wait seen after a successful probe exit.
        let result = receiver.recv_timeout(Duration::from_secs(2));
        writer.flush().unwrap();
        drop(writer);
        reader.join().unwrap();
        assert!(
            result
                .expect("complete proof must not wait for EOF")
                .unwrap()
                .verified()
        );
    }
}
