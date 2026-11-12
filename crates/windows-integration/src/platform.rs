use crate::ProxyBackend;
use std::path::Path;
use xray_model::{Error, ProxySnapshot, Result};

pub struct SystemProxy;
#[cfg(not(windows))]
impl ProxyBackend for SystemProxy {
    fn read(&self) -> Result<ProxySnapshot> {
        Ok(ProxySnapshot::default())
    }
    fn write(&mut self, _: &ProxySnapshot) -> Result<()> {
        Err(Error::new(
            "UNSUPPORTED_PLATFORM",
            "System proxy management is available on Windows only",
        ))
    }
}
#[cfg(windows)]
impl ProxyBackend for SystemProxy {
    fn read(&self) -> Result<ProxySnapshot> {
        unsafe {
            use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinInet::*};
            let keys = [
                INTERNET_PER_CONN_FLAGS,
                INTERNET_PER_CONN_PROXY_SERVER,
                INTERNET_PER_CONN_PROXY_BYPASS,
                INTERNET_PER_CONN_AUTOCONFIG_URL,
            ];
            let mut options: [INTERNET_PER_CONN_OPTIONW; 4] = std::mem::zeroed();
            for (o, k) in options.iter_mut().zip(keys) {
                o.dwOption = k;
            }
            let mut list: INTERNET_PER_CONN_OPTION_LISTW = std::mem::zeroed();
            list.dwSize = std::mem::size_of_val(&list) as u32;
            list.dwOptionCount = 4;
            list.pOptions = options.as_mut_ptr();
            let mut size = list.dwSize;
            if InternetQueryOptionW(
                std::ptr::null(),
                INTERNET_OPTION_PER_CONNECTION_OPTION,
                (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
                &mut size,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            fn string(p: *mut u16) -> String {
                if p.is_null() {
                    return String::new();
                }
                unsafe {
                    let mut n = 0;
                    while *p.add(n) != 0 {
                        n += 1;
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
                }
            }
            let value = ProxySnapshot {
                flags: options[0].Value.dwValue,
                server: string(options[1].Value.pszValue),
                bypass: string(options[2].Value.pszValue),
                pac_url: string(options[3].Value.pszValue),
            };
            for o in &options[1..] {
                if !o.Value.pszValue.is_null() {
                    GlobalFree(o.Value.pszValue.cast());
                }
            }
            Ok(value)
        }
    }
    fn write(&mut self, s: &ProxySnapshot) -> Result<()> {
        unsafe {
            use windows_sys::Win32::Networking::WinInet::*;
            let server = xray_config::security::wide(&s.server);
            let bypass = xray_config::security::wide(&s.bypass);
            let pac = xray_config::security::wide(&s.pac_url);
            let mut options: [INTERNET_PER_CONN_OPTIONW; 4] = std::mem::zeroed();
            options[0].dwOption = INTERNET_PER_CONN_FLAGS;
            options[0].Value.dwValue = s.flags;
            options[1].dwOption = INTERNET_PER_CONN_PROXY_SERVER;
            options[1].Value.pszValue = server.as_ptr() as *mut _;
            options[2].dwOption = INTERNET_PER_CONN_PROXY_BYPASS;
            options[2].Value.pszValue = bypass.as_ptr() as *mut _;
            options[3].dwOption = INTERNET_PER_CONN_AUTOCONFIG_URL;
            options[3].Value.pszValue = pac.as_ptr() as *mut _;
            let mut list: INTERNET_PER_CONN_OPTION_LISTW = std::mem::zeroed();
            list.dwSize = std::mem::size_of_val(&list) as u32;
            list.dwOptionCount = 4;
            list.pOptions = options.as_mut_ptr();
            if InternetSetOptionW(
                std::ptr::null(),
                INTERNET_OPTION_PER_CONNECTION_OPTION,
                (&list as *const INTERNET_PER_CONN_OPTION_LISTW).cast(),
                list.dwSize,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            for flag in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
                if InternetSetOptionW(std::ptr::null(), flag, std::ptr::null(), 0) == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            Ok(())
        }
    }
}

pub struct SystemStartup;
#[cfg(not(windows))]
impl crate::startup::StartupBackend for SystemStartup {
    fn read(&self, _: &str) -> Result<Option<crate::startup::StartupTask>> {
        Ok(None)
    }
    fn prepare(&self, _: &str, _: &Path, _: &Path) -> Result<crate::startup::StartupTask> {
        Err(Error::new(
            "UNSUPPORTED_PLATFORM",
            "Login startup is available on Windows only",
        ))
    }
    fn replace(
        &mut self,
        _: &str,
        before: &Option<crate::startup::StartupTask>,
        target: &Option<crate::startup::StartupTask>,
    ) -> Result<()> {
        if before.is_none() && target.is_none() {
            Ok(())
        } else {
            Err(Error::new(
                "UNSUPPORTED_PLATFORM",
                "Login startup is available on Windows only",
            ))
        }
    }
}
#[cfg(windows)]
impl SystemStartup {
    fn invoke(value: serde_json::Value) -> Result<serde_json::Value> {
        use std::{
            io::Write,
            os::windows::process::CommandExt,
            process::{Command, Stdio},
        };
        let windows = std::env::var_os("SystemRoot")
            .ok_or_else(|| Error::new("STARTUP_FAILED", "Missing SystemRoot"))?;
        let mut child = Command::new(
            Path::new(&windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        )
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("startup.ps1"),
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&value)?)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::new(
                if stderr.contains("STARTUP_CONFLICT") {
                    "STARTUP_CONFLICT"
                } else {
                    "STARTUP_FAILED"
                },
                "Task Scheduler rejected the operation; inspect current-user task ownership and permissions",
            ));
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }
}
#[cfg(windows)]
impl crate::startup::StartupBackend for SystemStartup {
    fn read(&self, name: &str) -> Result<Option<crate::startup::StartupTask>> {
        Ok(serde_json::from_value(Self::invoke(
            serde_json::json!({"op":"read", "name":name}),
        )?)?)
    }
    fn prepare(&self, owner: &str, exe: &Path, root: &Path) -> Result<crate::startup::StartupTask> {
        let exe = std::fs::canonicalize(exe)?;
        let exe = exe.to_string_lossy();
        let exe = exe.strip_prefix(r"\\?\").unwrap_or(&exe);
        let root = root.to_string_lossy();
        let root = root.strip_prefix(r"\\?\").unwrap_or(&root);
        Ok(serde_json::from_value(Self::invoke(
            serde_json::json!({"op":"prepare", "owner":owner, "exe":exe, "root":root, "sid":xray_config::security::principal()?}),
        )?)?)
    }
    fn replace(
        &mut self,
        name: &str,
        before: &Option<crate::startup::StartupTask>,
        target: &Option<crate::startup::StartupTask>,
    ) -> Result<()> {
        Self::invoke(
            serde_json::json!({"op":"replace", "name":name, "before":before, "target":target}),
        )?;
        Ok(())
    }
}

/// Read-only WinHTTP session proxy diagnostics. Never modifies WinHTTP.
#[cfg(windows)]
pub fn winhttp_snapshot() -> Result<xray_model::WinHttpConfig> {
    unsafe {
        use std::slice;
        use windows_sys::Win32::{
            Foundation::GlobalFree,
            Networking::WinHttp::{
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
                WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_OPTION_PROXY, WINHTTP_PROXY_INFO,
                WinHttpCloseHandle, WinHttpOpen, WinHttpQueryOption,
            },
        };
        fn take(p: windows_sys::core::PWSTR) -> String {
            if p.is_null() {
                return String::new();
            }
            unsafe {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                let value = String::from_utf16_lossy(slice::from_raw_parts(p, n));
                GlobalFree(p.cast());
                value
            }
        }
        let agent = xray_config::security::wide("XrayPilotDiagnostics");
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if session.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info: WINHTTP_PROXY_INFO = std::mem::zeroed();
        let mut length = std::mem::size_of::<WINHTTP_PROXY_INFO>() as u32;
        let ok = WinHttpQueryOption(
            session,
            WINHTTP_OPTION_PROXY,
            (&mut info as *mut WINHTTP_PROXY_INFO).cast(),
            &mut length,
        );
        WinHttpCloseHandle(session);
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let access_type = match info.dwAccessType {
            WINHTTP_ACCESS_TYPE_NAMED_PROXY => "NAMED_PROXY",
            WINHTTP_ACCESS_TYPE_NO_PROXY => "NO_PROXY",
            _ => "DEFAULT_PROXY",
        };
        Ok(xray_model::WinHttpConfig {
            access_type: access_type.into(),
            proxy: take(info.lpszProxy),
            bypass: take(info.lpszProxyBypass),
        })
    }
}

#[cfg(not(windows))]
pub fn winhttp_snapshot() -> Result<xray_model::WinHttpConfig> {
    Err(Error::new(
        "UNSUPPORTED_PLATFORM",
        "WinHTTP diagnostics are available on Windows only",
    ))
}
