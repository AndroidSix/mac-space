use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use tauri::Emitter;

#[derive(Clone)]
pub struct Rule {
    pub id: &'static str,
    pub relative: &'static str,
    pub group: &'static str,
    pub blockers: &'static [&'static str],
}

impl Rule {
    pub fn path(&self) -> PathBuf {
        #[cfg(windows)]
        {
            return std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::home().join("AppData/Local"))
                .join(self.relative);
        }
        #[cfg(target_os = "macos")]
        {
            crate::home().join(self.relative)
        }
    }
}

#[cfg(windows)]
pub fn rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "pip",
            relative: "pip/Cache",
            group: "packages",
            blockers: &["pip", "pip3", "python", "python3"],
        },
        Rule {
            id: "npm",
            relative: "npm-cache/_cacache",
            group: "packages",
            blockers: &["node", "npm", "pnpm", "yarn"],
        },
        Rule {
            id: "go",
            relative: "go-build",
            group: "development",
            blockers: &["go", "compile"],
        },
        Rule {
            id: "chrome",
            relative: "Google/Chrome/User Data/Default/Cache",
            group: "browser",
            blockers: &["chrome"],
        },
        Rule {
            id: "edge",
            relative: "Microsoft/Edge/User Data/Default/Cache",
            group: "browser",
            blockers: &["msedge"],
        },
        Rule {
            id: "electron",
            relative: "electron/Cache",
            group: "packages",
            blockers: &["node", "electron", "npm", "pnpm", "yarn"],
        },
        Rule {
            id: "electron-builder",
            relative: "electron-builder/Cache",
            group: "packages",
            blockers: &["node", "electron-builder", "npm", "pnpm", "yarn"],
        },
    ]
}

#[cfg(target_os = "macos")]
pub fn rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "xcode",
            relative: "Library/Developer/Xcode/DerivedData",
            group: "development",
            blockers: &["Xcode", "xcodebuild", "XCBBuildService"],
        },
        Rule {
            id: "uv",
            relative: ".cache/uv",
            group: "packages",
            blockers: &["uv"],
        },
        Rule {
            id: "pip",
            relative: "Library/Caches/pip",
            group: "packages",
            blockers: &["pip", "pip3"],
        },
        Rule {
            id: "npm",
            relative: ".npm/_cacache",
            group: "packages",
            blockers: &["npm", "npm install", "npm ci"],
        },
        Rule {
            id: "go",
            relative: "Library/Caches/go-build",
            group: "development",
            blockers: &["go", "compile"],
        },
        Rule {
            id: "vscode-update",
            relative: "Library/Caches/com.microsoft.VSCode.ShipIt",
            group: "updates",
            blockers: &["ShipIt", "Code"],
        },
        Rule {
            id: "playwright",
            relative: "Library/Caches/ms-playwright",
            group: "packages",
            blockers: &["playwright", "chrome-headless-shell", "Chromium"],
        },
        Rule {
            id: "chrome",
            relative: "Library/Caches/Google/Chrome",
            group: "browser",
            blockers: &["Google Chrome", "Google Chrome Helper"],
        },
        Rule {
            id: "edge",
            relative: "Library/Caches/Microsoft Edge",
            group: "browser",
            blockers: &["Microsoft Edge", "Microsoft Edge Helper"],
        },
        Rule {
            id: "electron",
            relative: "Library/Caches/electron",
            group: "packages",
            blockers: &["npm", "pnpm", "yarn"],
        },
        Rule {
            id: "electron-builder",
            relative: "Library/Caches/electron-builder",
            group: "packages",
            blockers: &["electron-builder", "npm", "pnpm", "yarn"],
        },
    ]
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub group: String,
    pub bytes: u64,
    pub files: u64,
    pub errors: u64,
    pub blocked: Vec<String>,
    pub device: u64,
    pub inode: u64,
    #[serde(default)]
    pub is_directory: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryScan {
    pub root: String,
    pub entries: Vec<Entry>,
    pub scanned_at: u64,
    pub files: u64,
    pub errors: u64,
    pub skipped_mounts: u64,
    pub error_paths: Vec<String>,
    pub cancelled: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub caches: Vec<Entry>,
    pub cache_scanned_at: u64,
    pub directory: Option<DirectoryScan>,
    pub dirty: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watching: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    pub errors: u64,
}

struct FileMetadata {
    directory: bool,
    device: u64,
    inode: u64,
    links: u64,
    bytes: u64,
}
fn file_metadata(path: &Path) -> std::io::Result<FileMetadata> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::symlink_metadata(path)?;
        Ok(FileMetadata {
            directory: m.is_dir() && !m.file_type().is_symlink(),
            device: m.dev(),
            inode: m.ino(),
            links: m.nlink(),
            bytes: m.blocks().saturating_mul(512),
        })
    }
    #[cfg(windows)]
    {
        let file = crate::windows::open_file(path, false, false)?;
        let m = crate::windows::file_info(&file)?;
        if m.dwFileAttributes
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Ok(FileMetadata {
                directory: false,
                device: 0,
                inode: 0,
                links: 1,
                bytes: 0,
            });
        }
        let (device, inode) = crate::windows::identity(&m);
        Ok(FileMetadata {
            directory: m.dwFileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                != 0,
            device,
            inode,
            links: m.nNumberOfLinks as u64,
            bytes: crate::windows::allocated(&file)?,
        })
    }
}

struct Walker<'a> {
    cancel: &'a AtomicBool,
    app: Option<&'a tauri::AppHandle>,
    files: u64,
    bytes: u64,
    errors: u64,
    mounts: u64,
    seen: HashSet<(u64, u64)>,
    error_paths: Vec<String>,
    last_emit: Instant,
    economy: bool,
}

impl<'a> Walker<'a> {
    fn new(cancel: &'a AtomicBool, app: Option<&'a tauri::AppHandle>, economy: bool) -> Self {
        Self {
            cancel,
            app,
            files: 0,
            bytes: 0,
            errors: 0,
            mounts: 0,
            seen: HashSet::new(),
            error_paths: vec![],
            last_emit: Instant::now(),
            economy,
        }
    }

    fn error(&mut self, path: &Path) {
        self.errors += 1;
        if self.error_paths.len() < 8 {
            self.error_paths.push(path.to_string_lossy().into_owned());
        }
    }

    fn walk(&mut self, root: &Path, device: u64) -> u64 {
        let mut stack = vec![root.to_path_buf()];
        let mut total = 0_u64;
        while let Some(path) = stack.pop() {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let metadata = match file_metadata(&path) {
                Ok(value) => value,
                Err(_) => {
                    self.error(&path);
                    continue;
                }
            };
            if metadata.device != device {
                self.mounts += 1;
                continue;
            }
            if !metadata.directory
                && metadata.links > 1
                && !self.seen.insert((metadata.device, metadata.inode))
            {
                continue;
            }
            let bytes = metadata.bytes;
            total = total.saturating_add(bytes);
            self.bytes = self.bytes.saturating_add(bytes);
            self.files += 1;
            if metadata.directory {
                match fs::read_dir(&path) {
                    Ok(children) => {
                        for child in children {
                            match child {
                                Ok(child) => stack.push(child.path()),
                                Err(_) => self.error(&path),
                            }
                        }
                    }
                    Err(_) => self.error(&path),
                }
            }
            if self.files.is_multiple_of(1024) {
                if self.economy {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                if self.last_emit.elapsed().as_millis() >= 250 {
                    if let Some(app) = self.app {
                        let _ = app.emit(
                            "scan-progress",
                            Progress {
                                path: path.to_string_lossy().into_owned(),
                                files: self.files,
                                bytes: self.bytes,
                                errors: self.errors,
                            },
                        );
                    }
                    self.last_emit = Instant::now();
                }
            }
        }
        total
    }
}

#[cfg(target_os = "macos")]
pub fn process_names() -> Vec<String> {
    let mut names: Vec<String> = std::process::Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|s| Path::new(s.trim()).file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_else(|| vec!["unknown".into()]);
    if let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-axo", "args="])
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            for token in line.split_whitespace().take(4) {
                let name = Path::new(token)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                if matches!(
                    name.as_ref(),
                    "uv" | "pip" | "pip3" | "npm" | "pnpm" | "yarn" | "go"
                ) {
                    names.push(name.into_owned());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(windows)]
pub fn process_names() -> Vec<String> {
    let system =
        sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_processes(
            sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
        ));
    if system.processes().is_empty() {
        return vec!["unknown".into()];
    }
    system
        .processes()
        .values()
        .flat_map(|p| {
            std::iter::once(p.name()).chain(p.cmd().iter().take(4).map(|v| v.as_os_str()))
        })
        .map(|value| {
            Path::new(value)
                .file_stem()
                .unwrap_or(value)
                .to_string_lossy()
                .to_lowercase()
        })
        .collect()
}

pub fn blockers(rule: &Rule) -> Vec<String> {
    process_names()
        .into_iter()
        .filter(|name| {
            rule.blockers
                .iter()
                .any(|blocked| name == blocked || name.starts_with(&format!("{blocked} (")))
                || name == "unknown"
        })
        .collect()
}

pub fn scan_caches(
    app: Option<&tauri::AppHandle>,
    cancel: &AtomicBool,
    economy: bool,
    selected: &HashSet<String>,
) -> Vec<Entry> {
    let mut results = vec![];
    for rule in rules() {
        if !selected.contains(rule.id) {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = rule.path();
        let Ok(metadata) = file_metadata(&path) else {
            continue;
        };
        if !metadata.directory {
            continue;
        }
        let mut walker = Walker::new(cancel, app, economy);
        let bytes = walker.walk(&path, metadata.device);
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        results.push(Entry {
            id: rule.id.into(),
            name: rule.id.into(),
            path: path.to_string_lossy().into_owned(),
            group: rule.group.into(),
            bytes,
            files: walker.files,
            errors: walker.errors,
            blocked: blockers(&rule),
            device: metadata.device,
            inode: metadata.inode,
            is_directory: true,
        });
    }
    results.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    results
}

pub fn scan_directory(
    root: PathBuf,
    app: Option<&tauri::AppHandle>,
    cancel: &AtomicBool,
    economy: bool,
) -> Result<DirectoryScan, String> {
    let metadata = file_metadata(&root).map_err(|e| format!("scan_failed: {e}"))?;
    if !metadata.directory {
        return Err("invalid_directory".into());
    }
    let mut walker = Walker::new(cancel, app, economy);
    let children = fs::read_dir(&root).map_err(|e| format!("permission_denied: {e}"))?;
    let mut entries = vec![];
    for child in children {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let child = match child {
            Ok(value) => value,
            Err(_) => {
                walker.error(&root);
                continue;
            }
        };
        let path = child.path();
        let before_errors = walker.errors;
        let before_files = walker.files;
        let bytes = walker.walk(&path, metadata.device);
        entries.push(Entry {
            name: child.file_name().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            bytes,
            files: walker.files - before_files,
            errors: walker.errors - before_errors,
            is_directory: file_metadata(&path).is_ok_and(|m| m.directory),
            ..Entry::default()
        });
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    Ok(DirectoryScan {
        root: root.to_string_lossy().into_owned(),
        entries,
        scanned_at: crate::now(),
        files: walker.files,
        errors: walker.errors,
        skipped_mounts: walker.mounts,
        error_paths: walker.error_paths,
        cancelled: cancel.load(Ordering::Relaxed),
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn scan_does_not_follow_symlinks_and_counts_hardlinks_once() {
        let root = std::env::temp_dir().join(format!("macspace-scan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("original"), vec![5_u8; 16384]).unwrap();
        fs::hard_link(root.join("original"), root.join("hardlink")).unwrap();
        std::os::unix::fs::symlink("/", root.join("outside")).unwrap();
        let result = scan_directory(root.clone(), None, &AtomicBool::new(false), false).unwrap();
        assert_eq!(result.files, 2);
        assert!(result.entries.iter().map(|e| e.bytes).sum::<u64>() < 32768);
        fs::remove_dir_all(root).unwrap();
    }
}
