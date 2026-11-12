use super::CleanReport;
use crate::{
    scanner::{self, Entry},
    windows,
};

pub fn clean(entry: &Entry) -> Result<CleanReport, String> {
    let rule = scanner::rules()
        .into_iter()
        .find(|rule| rule.id == entry.id)
        .ok_or("invalid_target")?;
    if entry.errors > 0 {
        return Err("scan_incomplete".into());
    }
    if !scanner::blockers(&rule).is_empty() {
        return Err("target_busy".into());
    }
    let path = rule.path();
    let pins = windows::pin_directory(&path).map_err(|_| "target_changed")?;
    let info =
        windows::file_info(pins.last().ok_or("invalid_target")?).map_err(|_| "target_changed")?;
    if windows::identity(&info) != (entry.device, entry.inode) {
        return Err("target_changed".into());
    }
    let before = crate::monitor::disk_space(&path).1;
    windows::clear_directory(&path, entry.device).map_err(|e| format!("cleanup_failed: {e}"))?;
    Ok(CleanReport {
        id: entry.id.clone(),
        available_change: crate::monitor::disk_space(&path).1 as i64 - before as i64,
        estimated_bytes: entry.bytes,
    })
}
