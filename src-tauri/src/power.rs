use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerMode {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerStatus {
    pub current: String,
    pub modes: Vec<PowerMode>,
    pub requires_admin: bool,
}

pub struct PowerMonitor {
    last: PowerStatus,
    refreshed: Option<Instant>,
}

impl PowerMonitor {
    pub fn new() -> Self {
        Self {
            last: PowerStatus::default(),
            refreshed: None,
        }
    }
    pub fn status(&mut self, force: bool) -> PowerStatus {
        if force
            || self
                .refreshed
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(30))
        {
            self.last = read().unwrap_or_default();
            self.refreshed = Some(Instant::now());
        }
        self.last.clone()
    }
}

#[cfg(target_os = "macos")]
pub fn read() -> Result<PowerStatus, String> {
    let output = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "cap"])
        .output()
        .map_err(|_| "power_unavailable")?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == "lowpowermode")
    {
        return Err("power_unavailable".into());
    }
    unsafe extern "C" {
        fn macspace_low_power_mode() -> i32;
    }
    Ok(PowerStatus {
        current: if unsafe { macspace_low_power_mode() } != 0 {
            "low"
        } else {
            "auto"
        }
        .into(),
        modes: vec![
            PowerMode {
                id: "auto".into(),
                name: "powerAuto".into(),
            },
            PowerMode {
                id: "low".into(),
                name: "powerLow".into(),
            },
        ],
        requires_admin: true,
    })
}

#[cfg(target_os = "macos")]
fn apply(id: &str) -> Result<(), String> {
    let low = match id {
        "auto" => 0,
        "low" => 1,
        _ => return Err("invalid_action".into()),
    };
    unsafe extern "C" {
        fn systembalance_set_low_power(low: i32) -> i32;
    }
    match unsafe { systembalance_set_low_power(low) } {
        0 => Ok(()),
        -128 => Err("power_cancelled".into()),
        _ => Err("power_denied".into()),
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::{
        Win32::{Foundation::LocalFree, System::Power::*},
        core::GUID,
    };
    fn guid_id(guid: &GUID) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
            guid.data1,
            guid.data2,
            guid.data3,
            guid.data4[0],
            guid.data4[1],
            guid.data4[2..]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }
    fn schemes() -> Vec<(GUID, String)> {
        let mut values = vec![];
        for index in 0..256 {
            let mut guid = GUID::from_u128(0);
            let mut size = std::mem::size_of::<GUID>() as u32;
            let result = unsafe {
                PowerEnumerate(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    ACCESS_SCHEME,
                    index,
                    (&mut guid as *mut GUID).cast(),
                    &mut size,
                )
            };
            if result != 0 {
                break;
            }
            let mut name = [0u16; 512];
            let mut bytes = std::mem::size_of_val(&name) as u32;
            let result = unsafe {
                PowerReadFriendlyName(
                    std::ptr::null_mut(),
                    &guid,
                    std::ptr::null(),
                    std::ptr::null(),
                    name.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            let name = if result == 0 {
                String::from_utf16_lossy(
                    &name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())],
                )
            } else {
                guid_id(&guid)
            };
            values.push((guid, name));
        }
        values
    }
    pub fn read() -> Result<PowerStatus, String> {
        let mut active = std::ptr::null_mut();
        if unsafe { PowerGetActiveScheme(std::ptr::null_mut(), &mut active) } != 0
            || active.is_null()
        {
            return Err("power_unavailable".into());
        }
        let current = guid_id(unsafe { &*active });
        unsafe {
            LocalFree(active.cast());
        }
        Ok(PowerStatus {
            current,
            modes: schemes()
                .into_iter()
                .map(|(guid, name)| PowerMode {
                    id: guid_id(&guid),
                    name,
                })
                .collect(),
            requires_admin: false,
        })
    }
    pub fn apply(id: &str) -> Result<(), String> {
        let (guid, _) = schemes()
            .into_iter()
            .find(|(guid, _)| guid_id(guid) == id)
            .ok_or("invalid_action")?;
        if unsafe { PowerSetActiveScheme(std::ptr::null_mut(), &guid) } != 0 {
            return Err("power_denied".into());
        }
        Ok(())
    }
}
#[cfg(windows)]
use windows::{apply, read};

#[tauri::command]
pub async fn get_power_status() -> Result<PowerStatus, String> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .map_err(|_| "power_unavailable".to_string())?
}

#[tauri::command]
pub async fn set_power_mode(
    state: tauri::State<'_, std::sync::Arc<crate::AppState>>,
    id: String,
) -> Result<PowerStatus, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !read()?.modes.iter().any(|mode| mode.id == id) {
            return Err("invalid_action".into());
        }
        apply(&id)?;
        let status = read()?;
        if status.current != id {
            return Err("power_not_applied".into());
        }
        {
            let mut monitor = state.monitor.lock().map_err(|_| "state_error")?;
            monitor.last.power = monitor.power.status(true);
        }
        state.api.publish(&state);
        Ok(status)
    })
    .await
    .map_err(|_| "power_unavailable".to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_power_mode_is_rejected_without_changing_system() {
        assert!(super::apply("arbitrary;command").is_err());
    }
}
