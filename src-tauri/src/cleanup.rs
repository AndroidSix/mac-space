use crate::scanner::{self, Entry};
use serde::Serialize;
use std::{
    ffi::{CStr, CString},
    fs, io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    pub id: String,
    pub available_change: i64,
    pub estimated_bytes: u64,
}

#[derive(Serialize)]
pub struct CleanupItem {
    pub id: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReport {
    pub items: Vec<CleanupItem>,
    pub cancelled: bool,
    pub available_change: i64,
    pub persistence_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct CleanupProgress {
    pub id: String,
    pub completed: usize,
    pub total: usize,
}

pub fn select_entries(ids: &[String], entries: &[Entry]) -> Result<Vec<Entry>, String> {
    let rules = scanner::rules();
    let mut seen = std::collections::HashSet::new();
    if ids.is_empty() || ids.len() > rules.len() {
        return Err("invalid_selection".into());
    }
    ids.iter()
        .map(|id| {
            if !seen.insert(id) || !rules.iter().any(|rule| rule.id == id) {
                return Err("invalid_selection".into());
            }
            entries
                .iter()
                .find(|entry| &entry.id == id)
                .cloned()
                .ok_or("scan_required".into())
        })
        .collect()
}

pub fn run_batch(
    entries: &[Entry],
    cancel: &AtomicBool,
    mut clean_entry: impl FnMut(&Entry) -> Result<CleanReport, String>,
    mut progress: impl FnMut(CleanupProgress),
) -> BatchReport {
    let mut report = BatchReport {
        items: vec![],
        cancelled: false,
        available_change: 0,
        persistence_error: None,
    };
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(CleanupProgress {
            id: entry.id.clone(),
            completed: report.items.len(),
            total: entries.len(),
        });
        report.items.push(CleanupItem {
            id: entry.id.clone(),
            error: clean_entry(entry).err(),
        });
        progress(CleanupProgress {
            id: entry.id.clone(),
            completed: report.items.len(),
            total: entries.len(),
        });
    }
    report.cancelled = report.items.len() < entries.len();
    report
}

fn open_directory(path: &Path) -> io::Result<OwnedFd> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_fd(fd: &OwnedFd) -> io::Result<libc::stat> {
    let mut metadata = std::mem::MaybeUninit::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), metadata.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { metadata.assume_init() })
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn clear_fd(fd: &OwnedFd, device: i32) -> io::Result<()> {
    let duplicate = unsafe { libc::dup(fd.as_raw_fd()) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe {
            libc::close(duplicate);
        }
        return Err(io::Error::last_os_error());
    }
    let stream = DirectoryStream(stream);
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) {
                Ok(())
            } else {
                Err(error)
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_owned();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let mut metadata = std::mem::MaybeUninit::uninit();
        if unsafe {
            libc::fstatat(
                fd.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            if io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_dev != device || metadata.st_uid != unsafe { libc::getuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "foreign owner or mount",
            ));
        }
        let is_directory = metadata.st_mode & libc::S_IFMT == libc::S_IFDIR;
        if is_directory {
            let child = unsafe {
                libc::openat(
                    fd.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if child < 0 {
                return Err(io::Error::last_os_error());
            }
            let child = unsafe { OwnedFd::from_raw_fd(child) };
            let actual = stat_fd(&child)?;
            if actual.st_dev != metadata.st_dev || actual.st_ino != metadata.st_ino {
                return Err(io::Error::other("directory changed during cleanup"));
            }
            clear_fd(&child, device)?;
        }
        if unsafe {
            libc::unlinkat(
                fd.as_raw_fd(),
                name.as_ptr(),
                if is_directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } < 0
            && io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT)
        {
            return Err(io::Error::last_os_error());
        }
    }
}

fn tool(name: &str) -> Option<PathBuf> {
    [
        crate::home().join(".local/bin").join(name),
        Path::new("/opt/homebrew/bin").join(name),
        Path::new("/usr/local/bin").join(name),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

pub fn clean(entry: &Entry) -> Result<CleanReport, String> {
    let rule = scanner::rules()
        .into_iter()
        .find(|rule| rule.id == entry.id)
        .ok_or("invalid_target")?;
    let path = crate::home().join(rule.relative);
    let metadata = fs::symlink_metadata(&path).map_err(|e| format!("target_changed: {e}"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || path.canonicalize().ok().as_ref() != Some(&path)
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.dev() != entry.device
        || metadata.ino() != entry.inode
    {
        return Err("target_changed".into());
    }
    if entry.errors > 0 {
        return Err("scan_incomplete".into());
    }
    if !scanner::blockers(&rule).is_empty() {
        return Err("target_busy".into());
    }
    let fd = open_directory(&path).map_err(|e| format!("permission_denied: {e}"))?;
    let actual = stat_fd(&fd).map_err(|e| format!("cleanup_failed: {e}"))?;
    if actual.st_ino != entry.inode || actual.st_dev as u64 != entry.device {
        return Err("target_changed".into());
    }
    let before = crate::monitor::disk_space(&path).1;
    let command = match rule.id {
        "uv" => Some((
            tool("uv").ok_or("tool_unavailable")?,
            vec![
                "cache",
                "clean",
                "--cache-dir",
                path.to_str().ok_or("invalid_target")?,
            ],
        )),
        "npm" => tool("npm").map(|bin| {
            (
                bin,
                vec![
                    "cache",
                    "clean",
                    "--force",
                    "--cache",
                    crate::home_str_npm(),
                ],
            )
        }),
        _ => None,
    };
    if let Some((binary, args)) = command {
        let output = Command::new(binary)
            .args(args)
            .env("PATH", "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
            .output()
            .map_err(|e| format!("cleanup_failed: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "cleanup_failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(800)
                    .collect::<String>()
            ));
        }
    } else {
        clear_fd(&fd, actual.st_dev).map_err(|e| format!("cleanup_failed: {e}"))?;
    }
    let after = crate::monitor::disk_space(&crate::home()).1;
    Ok(CleanReport {
        id: entry.id.clone(),
        available_change: after as i64 - before as i64,
        estimated_bytes: entry.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_validates_selection_continues_after_failure_and_stops_between_items() {
        let entries: Vec<_> = ["pip", "npm", "go"]
            .iter()
            .map(|id| Entry {
                id: id.to_string(),
                ..Default::default()
            })
            .collect();
        let ids = |values: &[&str]| values.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert!(select_entries(&[], &entries).is_err());
        assert!(select_entries(&ids(&["pip", "pip"]), &entries).is_err());
        assert!(select_entries(&ids(&["pip", "/tmp"]), &entries).is_err());
        assert!(select_entries(&ids(&["uv"]), &entries).is_err());
        let ordered = select_entries(&ids(&["npm", "pip", "go"]), &entries).unwrap();
        assert_eq!(ordered[0].id, "npm");
        let cancel = AtomicBool::new(false);
        let mut calls = vec![];
        let mut updates = vec![];
        let report = run_batch(
            &ordered,
            &cancel,
            |entry| {
                calls.push(entry.id.clone());
                if entry.id == "npm" {
                    return Err("target_busy".into());
                }
                cancel.store(true, Ordering::Relaxed);
                Ok(CleanReport {
                    id: entry.id.clone(),
                    available_change: 0,
                    estimated_bytes: 0,
                })
            },
            |progress| updates.push(progress.completed),
        );
        assert_eq!(calls, ["npm", "pip"]);
        assert_eq!(report.items[0].error.as_deref(), Some("target_busy"));
        assert!(report.items[1].error.is_none());
        assert!(report.cancelled);
        assert_eq!(updates, [0, 1, 1, 2]);
        assert!(
            run_batch(
                &ordered,
                &cancel,
                |_| panic!("cancelled batch must not clean"),
                |_| {}
            )
            .items
            .is_empty()
        );
    }
    #[test]
    fn cleanup_does_not_follow_symlinks_or_delete_root() {
        let root = std::env::temp_dir().join(format!("macspace-clean-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cache/nested")).unwrap();
        fs::write(root.join("keep"), b"important").unwrap();
        fs::write(root.join("cache/nested/remove"), b"cache").unwrap();
        std::os::unix::fs::symlink(root.join("keep"), root.join("cache/link")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("cache/outside")).unwrap();
        let fd = open_directory(&root.join("cache")).unwrap();
        clear_fd(&fd, stat_fd(&fd).unwrap().st_dev).unwrap();
        assert!(root.join("cache").is_dir());
        assert_eq!(fs::read_dir(root.join("cache")).unwrap().count(), 0);
        assert_eq!(fs::read(root.join("keep")).unwrap(), b"important");
        assert!(open_directory(&root.join("cache/link")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
