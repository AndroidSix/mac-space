use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Threading::*};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn open_external(url: &str) -> Result<(), String> {
    let url = wide(url);
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        Err("permission_denied".into())
    } else {
        Ok(())
    }
}
pub fn reveal(path: &str) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{path}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
pub fn system_language() -> &'static str {
    let mut locale = [0u16; 85];
    let count = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            locale.as_mut_ptr(),
            locale.len() as i32,
        )
    };
    if count > 0 && String::from_utf16_lossy(&locale[..count as usize]).starts_with("zh") {
        "zh"
    } else {
        "en"
    }
}
pub fn low_power() -> bool {
    let mut status = windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS::default();
    unsafe {
        windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut status) != 0
            && status.SystemStatusFlag == 1
    }
}
pub fn disk_space(path: &Path) -> (u64, u64) {
    let (mut available, mut total) = (0, 0);
    unsafe {
        GetDiskFreeSpaceExW(
            wide(path).as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        );
    }
    (total, available)
}
pub fn process_priority(pid: u32) -> i32 {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return 0;
    }
    let priority = unsafe { GetPriorityClass(handle) };
    unsafe {
        CloseHandle(handle);
    }
    if priority == BELOW_NORMAL_PRIORITY_CLASS || priority == IDLE_PRIORITY_CLASS {
        10
    } else {
        0
    }
}
pub fn process_action(pid: u32, start: u64, action: &str) -> Result<(), String> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err("permission_denied".into());
    }
    let result = (|| {
        let mut times = [FILETIME::default(); 4];
        let [created, exit, kernel, user] = &mut times;
        if unsafe { GetProcessTimes(handle, created, exit, kernel, user) } == 0 {
            return Err("process_gone".into());
        }
        let actual = ((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64)
            / 10_000_000
            - 11_644_473_600;
        if actual != start {
            return Err("process_changed".into());
        }
        match action {
            "lower" => {
                let priority = unsafe { GetPriorityClass(handle) };
                if priority == IDLE_PRIORITY_CLASS || priority == BELOW_NORMAL_PRIORITY_CLASS {
                    return Ok(());
                }
                if unsafe { SetPriorityClass(handle, BELOW_NORMAL_PRIORITY_CLASS) } == 0 {
                    Err("permission_denied".into())
                } else {
                    Ok(())
                }
            }
            "terminate" => {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                struct Request {
                    pid: u32,
                    sent: bool,
                }
                unsafe extern "system" fn close(hwnd: HWND, data: LPARAM) -> i32 {
                    let request = unsafe { &mut *(data as *mut Request) };
                    let mut owner = 0;
                    unsafe {
                        GetWindowThreadProcessId(hwnd, &mut owner);
                    }
                    if owner == request.pid
                        && unsafe { IsWindowVisible(hwnd) } != 0
                        && unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0
                    {
                        request.sent = true;
                    }
                    1
                }
                let mut request = Request { pid, sent: false };
                unsafe {
                    EnumWindows(Some(close), (&mut request as *mut Request) as isize);
                }
                if request.sent {
                    Ok(())
                } else {
                    Err("process_no_window".into())
                }
            }
            _ => Err("invalid_action".into()),
        }
    })();
    unsafe {
        CloseHandle(handle);
    }
    result
}

pub fn open_file(path: &Path, delete: bool, pin: bool) -> io::Result<File> {
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES
                | if pin { FILE_READ_DATA } else { 0 }
                | if delete { DELETE } else { 0 },
            FILE_SHARE_READ
                | if pin {
                    0
                } else {
                    FILE_SHARE_WRITE | FILE_SHARE_DELETE
                },
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}
pub fn file_info(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}
pub fn identity(info: &BY_HANDLE_FILE_INFORMATION) -> (u64, u64) {
    (
        info.dwVolumeSerialNumber as u64,
        (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64,
    )
}
pub fn allocated(file: &File) -> io::Result<u64> {
    let mut info = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut info as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info.AllocationSize.max(0) as u64)
}
pub fn pin_directory(path: &Path) -> io::Result<Vec<File>> {
    let mut pinned = vec![];
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let file = open_file(ancestor, false, true)?;
        let info = file_info(&file)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        {
            return Err(io::Error::other("reparse point or invalid directory"));
        }
        pinned.push(file);
    }
    Ok(pinned)
}
pub fn clear_directory(path: &Path, volume: u64) -> io::Result<()> {
    for child in std::fs::read_dir(path)? {
        let child = child?;
        let path = child.path();
        let file = open_file(&path, true, true)?;
        let info = file_info(&file)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || identity(&info).0 != volume
        {
            return Err(io::Error::other("reparse point or foreign volume"));
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            clear_directory(&path, volume)?;
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of_val(&disposition) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn protect(bytes: &[u8], encrypt: bool) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::*;
    let source = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        if encrypt {
            CryptProtectData(
                &source,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let value = if output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(value)
}

pub fn read_token() -> String {
    use windows_sys::Win32::Security::Credentials::*;
    let mut value = std::ptr::null_mut();
    if unsafe {
        CredReadW(
            wide("local.macspace.desktop.github").as_ptr(),
            CRED_TYPE_GENERIC,
            0,
            &mut value,
        )
    } == 0
    {
        return String::new();
    }
    let result = if value.is_null() {
        return String::new();
    } else if unsafe { (*value).CredentialBlobSize } == 0 {
        String::new()
    } else {
        unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(
                (*value).CredentialBlob,
                (*value).CredentialBlobSize as usize,
            ))
            .into_owned()
        }
    };
    unsafe {
        CredFree(value.cast());
    }
    result
}
pub fn write_token(value: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::*;
    let mut target = wide("local.macspace.desktop.github");
    if value.is_empty() {
        let result = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        return if result != 0 || unsafe { GetLastError() } == ERROR_NOT_FOUND {
            Ok(())
        } else {
            Err("keychain_failed".into())
        };
    }
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: value.len() as u32,
        CredentialBlob: value.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        Err("keychain_failed".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_encrypted_for_current_windows_user() {
        let encrypted = protect(b"test-secret", true).unwrap();
        assert!(!encrypted.windows(11).any(|v| v == b"test-secret"));
        assert_eq!(protect(&encrypted, false).unwrap(), b"test-secret");
    }
    #[test]
    fn cleanup_pins_root_and_rejects_junctions() {
        let root = std::env::temp_dir().join(format!("systembalance-clean-{}", std::process::id()));
        std::fs::create_dir_all(root.join("cache/nested")).unwrap();
        std::fs::write(root.join("cache/nested/remove"), b"cache").unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("outside/keep"), b"data").unwrap();
        let pins = pin_directory(&root.join("cache")).unwrap();
        let volume = identity(&file_info(pins.last().unwrap()).unwrap()).0;
        clear_directory(&root.join("cache"), volume).unwrap();
        assert!(root.join("cache").is_dir());
        assert!(std::fs::rename(root.join("cache"), root.join("moved")).is_err());
        let result = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("cache/link"))
            .arg(root.join("outside"))
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(pin_directory(&root.join("cache/link")).is_err());
        assert!(clear_directory(&root.join("cache"), volume).is_err());
        assert_eq!(std::fs::read(root.join("outside/keep")).unwrap(), b"data");
        drop(pins);
        std::fs::remove_dir_all(root).unwrap();
    }
}
