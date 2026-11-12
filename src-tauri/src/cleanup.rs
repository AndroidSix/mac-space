use crate::scanner::{self, Entry};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
#[path = "cleanup_macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "cleanup_windows.rs"]
mod platform;
pub use platform::clean;

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
}
