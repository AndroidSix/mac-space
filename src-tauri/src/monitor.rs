use serde::Serialize;
use std::{collections::HashSet, time::Instant};
use sysinfo::{Components, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn macspace_thermal_state() -> i32;
    fn macspace_low_power_mode() -> i32;
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensor {
    pub name: String,
    pub celsius: f32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    pub start_time: u64,
    pub manageable: bool,
    pub nice: i32,
    pub lowered: bool,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub cpu: f32,
    pub cores: usize,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_available: u64,
    pub thermal_state: i32,
    pub low_power: bool,
    pub sensors: Vec<Sensor>,
    pub processes: Vec<ProcessInfo>,
    pub process_count: usize,
    pub load_average: [f64; 3],
    pub sampled_at: u64,
    pub chip: String,
    pub os: String,
    pub platform: String,
    pub power: crate::power::PowerStatus,
}

pub struct Monitor {
    system: System,
    components: Components,
    last_refresh: Instant,
    pub last: Metrics,
    pub power: crate::power::PowerMonitor,
}

fn process_refresh() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_user(UpdateKind::OnlyIfNotSet)
}

impl Monitor {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_usage();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        Self {
            system,
            components: Components::new_with_refreshed_list(),
            last_refresh: Instant::now() - std::time::Duration::from_secs(2),
            last: Metrics::default(),
            power: crate::power::PowerMonitor::new(),
        }
    }

    pub fn sample(&mut self) -> Metrics {
        if self.last.sampled_at != 0 && self.last_refresh.elapsed().as_millis() < 1000 {
            return self.last.clone();
        }
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        self.components.refresh(false);
        let mut protected = HashSet::new();
        let mut current = Some(Pid::from_u32(std::process::id()));
        while let Some(pid) = current {
            if !protected.insert(pid) {
                break;
            }
            current = self.system.process(pid).and_then(|p| p.parent());
        }
        let own_id = self
            .system
            .process(Pid::from_u32(std::process::id()))
            .and_then(|p| p.user_id())
            .cloned();
        let mut processes: Vec<_> = self
            .system
            .processes()
            .iter()
            .map(|(pid, p)| {
                let own = own_id.as_ref().is_some_and(|id| p.user_id() == Some(id));
                let name = p.name().to_string_lossy().into_owned();
                let manageable = own
                    && pid.as_u32() > 100
                    && !protected.contains(pid)
                    && !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "explorer.exe"
                            | "dwm.exe"
                            | "winlogon.exe"
                            | "csrss.exe"
                            | "services.exe"
                            | "svchost.exe"
                            | "lsass.exe"
                            | "sihost.exe"
                            | "taskhostw.exe"
                            | "searchhost.exe"
                            | "startmenuexperiencehost.exe"
                            | "shellexperiencehost.exe"
                    )
                    && !matches!(
                        name.as_str(),
                        "Finder"
                            | "Dock"
                            | "SystemUIServer"
                            | "loginwindow"
                            | "WindowServer"
                            | "launchd"
                            | "runningboardd"
                            | "cfprefsd"
                            | "tccd"
                            | "securityd"
                    );
                let nice = priority(pid.as_u32());
                ProcessInfo {
                    pid: pid.as_u32(),
                    name,
                    cpu: p.cpu_usage(),
                    memory: p.memory(),
                    start_time: p.start_time(),
                    manageable,
                    nice,
                    lowered: nice >= 10,
                }
            })
            .collect();
        processes.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
        let mut sensors: Vec<_> = self
            .components
            .iter()
            .filter_map(|c| {
                c.temperature()
                    .filter(|t| t.is_finite() && *t > 0.0 && *t < 150.0)
                    .map(|celsius| Sensor {
                        name: c.label().to_string(),
                        celsius,
                    })
            })
            .collect();
        sensors.sort_by(|a, b| b.celsius.total_cmp(&a.celsius));
        let (disk_total, disk_available) = disk_space(&crate::home());
        let load = System::load_average();
        let process_count = processes.len();
        self.last = Metrics {
            cpu: self.system.global_cpu_usage(),
            cores: self.system.cpus().len(),
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            swap_used: self.system.used_swap(),
            disk_total,
            disk_available,
            thermal_state: thermal_state(),
            low_power: low_power(),
            sensors,
            processes,
            process_count,
            load_average: [load.one, load.five, load.fifteen],
            sampled_at: crate::now(),
            chip: self
                .system
                .cpus()
                .first()
                .map(|c| c.brand().to_string())
                .unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            platform: std::env::consts::OS.into(),
            power: self.power.status(false),
        };
        self.last_refresh = Instant::now();
        self.last.clone()
    }
}

fn thermal_state() -> i32 {
    #[cfg(target_os = "macos")]
    {
        unsafe { macspace_thermal_state() }
    }
    #[cfg(windows)]
    {
        -1
    }
}
fn low_power() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { macspace_low_power_mode() != 0 }
    }
    #[cfg(windows)]
    {
        crate::windows::low_power()
    }
}
fn priority(pid: u32) -> i32 {
    #[cfg(target_os = "macos")]
    {
        unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) }
    }
    #[cfg(windows)]
    {
        crate::windows::process_priority(pid)
    }
}
pub fn disk_space(path: &std::path::Path) -> (u64, u64) {
    #[cfg(windows)]
    {
        crate::windows::disk_space(path)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return (0, 0);
        };
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return (0, 0);
        }
        let stat = unsafe { stat.assume_init() };
        (
            stat.f_blocks as u64 * stat.f_frsize,
            stat.f_bavail as u64 * stat.f_frsize,
        )
    }
}

pub fn process_action(pid: u32, start_time: u64, action: &str) -> Result<(), String> {
    let mut monitor = Monitor::new();
    let sample = monitor.sample();
    let process = sample
        .processes
        .iter()
        .find(|p| p.pid == pid)
        .ok_or("process_gone")?;
    if process.start_time != start_time {
        return Err("process_changed".into());
    }
    if !process.manageable {
        return Err("process_protected".into());
    }
    #[cfg(windows)]
    {
        crate::windows::process_action(pid, start_time, action)
    }
    #[cfg(target_os = "macos")]
    {
        let result = match action {
            "lower" => unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, process.nice.max(10)) },
            "terminate" => unsafe { libc::kill(pid as i32, libc::SIGTERM) },
            _ => return Err("invalid_action".into()),
        };
        if result != 0 {
            return Err(format!(
                "permission_denied: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_metrics_have_real_capacity_and_protect_this_process() {
        let metrics = Monitor::new().sample();
        assert!(metrics.memory_total > 0 && metrics.disk_total > 0 && metrics.cores > 0);
        assert!(
            !metrics
                .processes
                .iter()
                .find(|p| p.pid == std::process::id())
                .unwrap()
                .manageable
        );
        println!("Temperature sensors: {}", metrics.sensors.len());
        for sensor in metrics.sensors.iter().take(5) {
            println!("{}: {:.1} C", sensor.name, sensor.celsius);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_control_checks_identity_and_handles_only_test_child() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("20")
            .spawn()
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let metrics = Monitor::new().sample();
            let process = metrics
                .processes
                .iter()
                .find(|p| p.pid == child.id())
                .unwrap();
            assert_eq!(
                process_action(child.id(), process.start_time + 1, "terminate"),
                Err("process_changed".into())
            );
            assert!(child.try_wait().unwrap().is_none());
            process_action(child.id(), process.start_time, "lower").unwrap();
            assert!(unsafe { libc::getpriority(libc::PRIO_PROCESS, child.id()) } >= 10);
            process_action(child.id(), process.start_time, "terminate").unwrap();
        }));
        let _ = child.kill();
        let _ = child.wait();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
