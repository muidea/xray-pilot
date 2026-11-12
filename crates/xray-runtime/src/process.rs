use crate::{Logs, log};
use xray_model::{Candidate, Result};

#[cfg(not(windows))]
pub struct Process(tokio::process::Child);
#[cfg(not(windows))]
impl Process {
    pub fn spawn(candidate: &Candidate, logs: Logs) -> Result<Self> {
        use tokio::io::AsyncReadExt;
        let mut cmd = tokio::process::Command::new(&candidate.core.path);
        cmd.args(["run", "-config"])
            .arg(&candidate.config_path)
            .current_dir(&candidate.resources.root)
            .env_clear()
            .envs(crate::core_environment(&candidate.resources.root))
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        fn drain<T: tokio::io::AsyncRead + Unpin + Send + 'static>(mut pipe: T, logs: Logs) {
            tokio::spawn(async move {
                let mut bytes = [0; 2048];
                let mut buffer = String::new();
                while let Ok(n) = pipe.read(&mut bytes).await {
                    if n == 0 {
                        break;
                    }
                    buffer.push_str(&String::from_utf8_lossy(&bytes[..n]));
                    while let Some(end) = buffer.find('\n') {
                        log(&logs, buffer.drain(..=end).collect::<String>());
                    }
                    if buffer.len() > 64 * 1024 {
                        log(&logs, std::mem::take(&mut buffer));
                    }
                }
                if !buffer.is_empty() {
                    log(&logs, buffer);
                }
            });
        }
        if let Some(out) = child.stdout.take() {
            drain(out, logs.clone());
        }
        if let Some(err) = child.stderr.take() {
            drain(err, logs);
        }
        Ok(Self(child))
    }
    pub fn try_wait(&mut self) -> Result<Option<i32>> {
        Ok(self.0.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }
    pub async fn stop(&mut self) -> Result<()> {
        if self.0.try_wait()?.is_none() {
            self.0.kill().await?;
        }
        Ok(())
    }
}

#[cfg(windows)]
pub struct Process {
    process: windows_sys::Win32::Foundation::HANDLE,
    job: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg(windows)]
unsafe impl Send for Process {}
#[cfg(windows)]
impl Process {
    pub fn spawn(candidate: &Candidate, logs: Logs) -> Result<Self> {
        use std::{
            os::windows::{ffi::OsStrExt, io::FromRawHandle},
            ptr::{null, null_mut},
        };
        use windows_sys::Win32::{
            Foundation::{
                CloseHandle, GENERIC_READ, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
                SetHandleInformation,
            },
            Security::SECURITY_ATTRIBUTES,
            Storage::FileSystem::{
                CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING,
            },
            System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
        };
        fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
            s.as_ref().encode_wide().chain(Some(0)).collect()
        }
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                CloseHandle(job);
                return Err(std::io::Error::last_os_error().into());
            }
            let mut read = null_mut();
            let mut write = null_mut();
            let security = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            if CreatePipe(&mut read, &mut write, &security, 0) == 0 {
                CloseHandle(job);
                return Err(std::io::Error::last_os_error().into());
            }
            if SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(read);
                CloseHandle(write);
                CloseHandle(job);
                return Err(error.into());
            }
            let input = CreateFileW(
                wide("NUL").as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &security,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            );
            if input == INVALID_HANDLE_VALUE {
                let error = std::io::Error::last_os_error();
                CloseHandle(read);
                CloseHandle(write);
                CloseHandle(job);
                return Err(error.into());
            }
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of_val(&startup) as u32;
            startup.dwFlags = STARTF_USESTDHANDLES;
            startup.hStdOutput = write;
            startup.hStdError = write;
            startup.hStdInput = input;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let mut line = wide(format!(
                "\"{}\" run -config \"{}\"",
                candidate.core.path.display(),
                candidate.config_path.display()
            ));
            // Use an explicit environment block; Core assets resolve to the frozen resource set.
            let mut env = crate::core_environment(&candidate.resources.root);
            env.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
            let mut environment: Vec<u16> = env
                .into_iter()
                .flat_map(|(k, v)| {
                    k.encode_wide()
                        .chain([b'=' as u16])
                        .chain(v.encode_wide())
                        .chain([0])
                        .collect::<Vec<_>>()
                })
                .collect();
            environment.push(0);
            let ok = CreateProcessW(
                wide(&candidate.core.path).as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                wide(&candidate.resources.root).as_ptr(),
                &startup,
                &mut pi,
            );
            CloseHandle(write);
            CloseHandle(input);
            if ok == 0 {
                CloseHandle(read);
                CloseHandle(job);
                return Err(std::io::Error::last_os_error().into());
            }
            if AssignProcessToJobObject(job, pi.hProcess) == 0
                || ResumeThread(pi.hThread) == u32::MAX
            {
                TerminateProcess(pi.hProcess, 1);
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
                CloseHandle(read);
                CloseHandle(job);
                return Err(std::io::Error::last_os_error().into());
            }
            CloseHandle(pi.hThread);
            let mut pipe = std::fs::File::from_raw_handle(read);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut bytes = [0; 2048];
                let mut buffer = String::new();
                while let Ok(n) = pipe.read(&mut bytes) {
                    if n == 0 {
                        break;
                    }
                    buffer.push_str(&String::from_utf8_lossy(&bytes[..n]));
                    while let Some(end) = buffer.find('\n') {
                        log(&logs, buffer.drain(..=end).collect::<String>());
                    }
                    if buffer.len() > 64 * 1024 {
                        log(&logs, std::mem::take(&mut buffer));
                    }
                }
                if !buffer.is_empty() {
                    log(&logs, buffer);
                }
            });
            Ok(Self {
                process: pi.hProcess,
                job,
            })
        }
    }
    pub fn try_wait(&mut self) -> Result<Option<i32>> {
        use windows_sys::Win32::System::Threading::*;
        unsafe {
            let result = WaitForSingleObject(self.process, 0);
            if result == 258 {
                return Ok(None);
            }
            if result != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut code = 0;
            if GetExitCodeProcess(self.process, &mut code) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Some(code as i32))
        }
    }
    pub async fn stop(&mut self) -> Result<()> {
        use windows_sys::Win32::System::{
            JobObjects::TerminateJobObject, Threading::WaitForSingleObject,
        };
        unsafe {
            if self.try_wait()?.is_none() {
                if TerminateJobObject(self.job, 0) == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                if WaitForSingleObject(self.process, 5000) != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
            windows_sys::Win32::Foundation::CloseHandle(self.process);
        }
    }
}
