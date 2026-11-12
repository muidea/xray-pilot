use std::path::Path;
use xray_model::Result;

#[cfg(windows)]
pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(unix)]
pub fn principal() -> Result<String> {
    // The socket and all data are inside an owner-only directory.
    Ok(std::fs::metadata("/proc/self")?.uid().to_string())
}
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(windows)]
pub fn principal() -> Result<String> {
    unsafe { process_principal(windows_sys::Win32::System::Threading::GetCurrentProcess()) }
}
#[cfg(windows)]
/// Returns the user SID associated with a process token.
///
/// # Safety
/// `process` must be a live process handle with token-query access and must remain open for this call.
pub unsafe fn process_principal(process: windows_sys::Win32::Foundation::HANDLE) -> Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::OpenProcessToken,
    };
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let result = (|| {
            let mut size = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut string = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut string) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut len = 0;
            while *string.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(string, len));
            LocalFree(string.cast());
            Ok(text)
        })();
        CloseHandle(token);
        result
    }
}

pub fn protect_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW},
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
                PROTECTED_DACL_SECURITY_INFORMATION,
            },
        };
        let descriptor = descriptor()?;
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) == 0 {
            LocalFree(descriptor);
            return Err(std::io::Error::last_os_error().into());
        }
        let error = SetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        );
        LocalFree(descriptor);
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32).into());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn descriptor() -> Result<*mut std::ffi::c_void> {
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    // The manager is a per-user application.  Do not grant SYSTEM or any
    // service account access: a machine service is outside this product's
    // deployment model.  Elevated processes for the same signed-in user are
    // still authorized by the SID check in the IPC peer validation.
    let sddl = wide(format!("D:P(A;OICI;FA;;;{})", principal()?));
    let mut descriptor = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(descriptor)
}
