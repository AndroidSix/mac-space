mod cleanup;
mod menu;
mod monitor;
mod scanner;

use notify::{RecursiveMode, Watcher};
use scanner::Snapshot;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME is required")
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn home_str_npm() -> &'static str {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| home().join(".npm").to_string_lossy().into_owned())
}

struct AppState {
    monitor: Mutex<monitor::Monitor>,
    snapshot: Mutex<Snapshot>,
    busy: AtomicBool,
    cancel: AtomicBool,
    dirty: AtomicBool,
    store: PathBuf,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    changed: Mutex<std::collections::HashSet<String>>,
    watching: AtomicBool,
}

impl AppState {
    fn persist(&self) -> Result<(), String> {
        let snapshot = self.snapshot.lock().map_err(|_| "state_error")?;
        let json = serde_json::to_vec(&*snapshot).map_err(|e| e.to_string())?;
        let temporary = self.store.with_extension("tmp");
        std::fs::write(&temporary, json)
            .and_then(|_| std::fs::rename(&temporary, &self.store))
            .map_err(|e| format!("save_failed: {e}"))
    }
}

struct BusyGuard(Arc<AppState>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

fn acquire(state: &Arc<AppState>) -> Result<BusyGuard, String> {
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "scan_busy")?;
    state.cancel.store(false, Ordering::Relaxed);
    Ok(BusyGuard(state.clone()))
}

fn worker_priority(economy: bool) {
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative: i32) -> i32;
    }
    unsafe {
        pthread_set_qos_class_self_np(if economy { 0x09 } else { 0x11 }, 0);
    }
}

#[tauri::command]
async fn get_metrics(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<monitor::Metrics, String> {
    let state = state.inner().clone();
    let hidden = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true));
    tauri::async_runtime::spawn_blocking(move || {
        let mut monitor = state.monitor.lock().map_err(|_| "state_error")?;
        if hidden && monitor.last.sampled_at != 0 {
            return Ok(monitor.last.clone());
        }
        Ok(monitor.sample())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_snapshot(state: State<'_, Arc<AppState>>) -> Result<Snapshot, String> {
    let mut snapshot = state.snapshot.lock().map_err(|_| "state_error")?.clone();
    snapshot.dirty = state.dirty.load(Ordering::Relaxed);
    snapshot.watching = state.watching.load(Ordering::Relaxed);
    Ok(snapshot)
}

#[tauri::command]
async fn start_scan(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    mode: String,
    root: Option<String>,
    economy: bool,
) -> Result<Snapshot, String> {
    let guard = acquire(state.inner())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        worker_priority(economy);
        if mode == "caches" || mode == "changed" {
            let selected = {
                let mut changed = state.changed.lock().map_err(|_| "state_error")?;
                if mode == "caches" {
                    changed.clear();
                    scanner::rules().iter().map(|r| r.id.to_string()).collect()
                } else {
                    std::mem::take(&mut *changed)
                }
            };
            state.dirty.store(false, Ordering::Relaxed);
            let caches = scanner::scan_caches(Some(&app), &state.cancel, economy, &selected);
            if state.cancel.load(Ordering::Relaxed) {
                state
                    .changed
                    .lock()
                    .map_err(|_| "state_error")?
                    .extend(selected);
                state.dirty.store(true, Ordering::Relaxed);
                return Err("scan_cancelled".into());
            }
            let mut snapshot = state.snapshot.lock().map_err(|_| "state_error")?;
            snapshot
                .caches
                .retain(|entry| !selected.contains(&entry.id));
            snapshot.caches.extend(caches);
            snapshot
                .caches
                .sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
            snapshot.cache_scanned_at = now();
        } else if mode == "directory" {
            let root = match root.as_deref() {
                Some("home") => home(),
                Some("disk") => PathBuf::from("/System/Volumes/Data"),
                Some(path) if Path::new(path).is_absolute() => PathBuf::from(path),
                _ => return Err("invalid_directory".into()),
            };
            let directory = scanner::scan_directory(root, Some(&app), &state.cancel, economy)?;
            state.snapshot.lock().map_err(|_| "state_error")?.directory = Some(directory);
        } else {
            return Err("invalid_action".into());
        }
        state.persist()?;
        let mut result = state.snapshot.lock().map_err(|_| "state_error")?.clone();
        result.dirty = state.dirty.load(Ordering::Relaxed);
        result.watching = state.watching.load(Ordering::Relaxed);
        let _ = app.emit("scan-complete", ());
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_scan(state: State<'_, Arc<AppState>>) {
    state.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn clean_cache(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<cleanup::CleanReport, String> {
    let guard = acquire(state.inner())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let entry = state
            .snapshot
            .lock()
            .map_err(|_| "state_error")?
            .caches
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or("scan_required")?;
        let result = cleanup::clean(&entry);
        state.dirty.store(true, Ordering::Relaxed);
        state
            .changed
            .lock()
            .map_err(|_| "state_error")?
            .insert(id.clone());
        if result.is_ok() {
            state
                .snapshot
                .lock()
                .map_err(|_| "state_error")?
                .caches
                .retain(|entry| entry.id != id);
            state.persist()?;
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn manage_process(pid: u32, start_time: u64, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || monitor::process_action(pid, start_time, &action))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clean_caches(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<cleanup::BatchReport, String> {
    let guard = acquire(state.inner())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        worker_priority(true);
        let entries = cleanup::select_entries(
            &ids,
            &state.snapshot.lock().map_err(|_| "state_error")?.caches,
        )?;
        let before = monitor::disk_space(&home()).1;
        let mut report = cleanup::run_batch(&entries, &state.cancel, cleanup::clean, |progress| {
            let _ = app.emit("cleanup-progress", progress);
        });
        state.dirty.store(true, Ordering::Relaxed);
        state.changed.lock().map_err(|_| "state_error")?.extend(ids);
        state
            .snapshot
            .lock()
            .map_err(|_| "state_error")?
            .caches
            .retain(|entry| {
                !report
                    .items
                    .iter()
                    .any(|item| item.id == entry.id && item.error.is_none())
            });
        report.persistence_error = state.persist().err();
        report.available_change = monitor::disk_space(&home()).1 as i64 - before as i64;
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn choose_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| {
                p.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    if !Path::new(&path).is_absolute() {
        return Err("invalid_directory".into());
    }
    std::process::Command::new("/usr/bin/open")
        .args(["-R", &path])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_permissions() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_system_language() -> &'static str {
    unsafe extern "C" {
        fn macspace_system_is_chinese() -> i32;
    }
    if unsafe { macspace_system_is_chinese() } == 1 {
        "zh"
    } else {
        "en"
    }
}

#[tauri::command]
fn set_ui_language(app: tauri::AppHandle, language: String) -> Result<(), String> {
    if language != "en" && language != "zh" {
        return Err("invalid_action".into());
    }
    menu::localize(&app, &language).map_err(|e| e.to_string())
}

fn watch_caches(app: &tauri::AppHandle, state: &Arc<AppState>) {
    let weak = Arc::downgrade(state);
    let handle = app.clone();
    let Ok(mut watcher) =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Some(state) = weak.upgrade() {
                let mut changed = state.changed.lock().unwrap_or_else(|e| e.into_inner());
                let before = changed.len();
                for rule in scanner::rules() {
                    let root = home().join(rule.relative);
                    let relevant = match &event {
                        Ok(e) => {
                            !e.kind.is_access()
                                && (e.need_rescan()
                                    || e.paths.iter().any(|path| {
                                        path.starts_with(&root) || root.starts_with(path)
                                    }))
                        }
                        Err(_) => true,
                    };
                    if relevant {
                        changed.insert(rule.id.to_string());
                    }
                }
                if !changed.is_empty()
                    && (changed.len() != before || !state.dirty.load(Ordering::Relaxed))
                {
                    state.dirty.store(true, Ordering::Relaxed);
                    let _ = handle.emit("cache-dirty", ());
                }
            }
        })
    else {
        return;
    };
    let mut paths = std::collections::HashSet::new();
    let mut watched = 0;
    for rule in scanner::rules() {
        let path = home().join(rule.relative);
        let existing = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(&path).to_path_buf()
        };
        if existing.is_dir()
            && paths.insert(existing.clone())
            && watcher.watch(&existing, RecursiveMode::Recursive).is_ok()
        {
            watched += 1;
        }
    }
    state.watching.store(watched > 0, Ordering::Relaxed);
    *state.watcher.lock().unwrap() = Some(watcher);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            menu::localize(app.handle(), get_system_language())?;
            let folder = app.path().app_data_dir()?;
            std::fs::create_dir_all(&folder)?;
            let store = folder.join("snapshot.json");
            let snapshot = std::fs::read(&store)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Snapshot>(&bytes).ok())
                .unwrap_or_default();
            let state = Arc::new(AppState {
                monitor: Mutex::new(monitor::Monitor::new()),
                snapshot: Mutex::new(snapshot),
                busy: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                dirty: AtomicBool::new(true),
                store,
                watcher: Mutex::new(None),
                changed: Mutex::new(scanner::rules().iter().map(|r| r.id.to_string()).collect()),
                watching: AtomicBool::new(false),
            });
            watch_caches(app.handle(), &state);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_metrics,
            get_snapshot,
            start_scan,
            cancel_scan,
            clean_cache,
            clean_caches,
            manage_process,
            choose_directory,
            reveal_path,
            open_permissions,
            get_system_language,
            set_ui_language
        ])
        .run(tauri::generate_context!())
        .expect("could not start MacSpace");
}
