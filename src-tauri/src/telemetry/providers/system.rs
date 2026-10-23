//! One persistent sysinfo owner; only CPU, usable RAM, process CPU/resident memory are refreshed.
use crate::telemetry::{
    bounds,
    model::*,
    normalize::{empty_cpu, empty_memory, ram_accounting},
    providers::*,
};
use std::{collections::HashMap, sync::Mutex};
use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System};

pub fn normalize_process_cpu(raw: f64, logical_cpus: usize) -> Option<f64> {
    if logical_cpus == 0 {
        return None;
    }
    let value = raw / logical_cpus as f64;
    valid_pct(&value).then_some(value)
}
#[derive(Default)]
struct State {
    system: System,
    last_elapsed: Option<u64>,
    process_starts: HashMap<u32, u64>,
    generation: u64,
}
#[derive(Default)]
pub struct SystemProvider {
    state: Mutex<State>,
}
impl SystemProvider {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Provider for SystemProvider {
    fn provider_id(&self) -> &'static str {
        "sysinfo"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.system.refresh_cpu_usage();
        state
            .system
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        state.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let ready = state
            .last_elapsed
            .is_some_and(|old| ctx.elapsed_ms > old && ctx.elapsed_ms - old <= 2 * ctx.interval_ms);
        state.last_elapsed = Some(ctx.elapsed_ms);
        state.generation = state.generation.saturating_add(1);
        let mut cpu = empty_cpu("cpu:system");
        let cpu_access = cpu_access();
        if ready && cpu_access.is_ok() {
            let v = f64::from(state.system.global_cpu_usage());
            cpu.total_pct = observed(
                valid_pct(&v).then_some(v).ok_or(ProviderError::Invalid),
                ctx,
                "cpu:system",
            );
        }
        if let Err(error) = cpu_access {
            cpu.total_pct = error.reading("cpu:system", "CPU counters are not readable");
        }
        let mut memory = empty_memory("host:system");
        let total = state.system.total_memory();
        let available = state.system.available_memory();
        let memory_access = memory_access();
        if let Some((used, pct)) =
            ram_accounting(total, available).filter(|_| memory_access.is_ok())
        {
            memory.total_bytes = observed(Ok(ByteCount(total)), ctx, "host:system");
            memory.available_bytes = observed(Ok(ByteCount(available)), ctx, "host:system");
            memory.used_bytes = observed(Ok(ByteCount(used)), ctx, "host:system");
            memory.used_pct = observed(Ok(pct), ctx, "host:system");
        }
        if let Err(error) = memory_access {
            memory.total_bytes = error.reading("host:system", "RAM counters are not readable");
            memory.available_bytes = memory.total_bytes.clone();
            memory.used_bytes = memory.total_bytes.clone();
            memory.used_pct = error.reading("host:system", "RAM counters are not readable");
        }
        let cores = state.system.cpus().len();
        let mut starts = HashMap::new();
        let processes = state
            .system
            .processes()
            .iter()
            .filter_map(|(pid, p)| {
                // Linux task rows share the leader's RSS: exclude kernel/user threads before
                // building either ranking input. Real child processes remain independent entries.
                // thread_kind() returns None on non-Linux platforms in sysinfo 0.33.1.
                if p.thread_kind().is_some() || p.memory() == 0 {
                    return None;
                }
                #[cfg(target_os = "linux")]
                if std::fs::read_to_string(format!("/proc/{}/stat", pid.as_u32())).is_err()
                    || std::fs::read_to_string(format!("/proc/{}/statm", pid.as_u32())).is_err()
                {
                    return None;
                }
                let pid = pid.as_u32();
                let start = p.start_time();
                let previous = state.process_starts.get(&pid).copied();
                starts.insert(pid, start);
                let key = if start > 0 {
                    format!("{pid}:{start}")
                } else {
                    format!("{pid}:generation-{}", state.generation)
                };
                let pct = normalize_process_cpu(f64::from(p.cpu_usage()), cores);
                Some(ProcessObservation {
                    key,
                    pid,
                    name: bounds::bounded_label(
                        &p.name().to_string_lossy(),
                        bounds::LABEL_MAX_BYTES,
                    ),
                    resident_bytes: observed(Ok(ByteCount(p.memory())), ctx, "host:system"),
                    cpu_pct: observed(
                        if ready && previous == Some(start) {
                            pct.ok_or(ProviderError::Invalid)
                        } else {
                            Err(ProviderError::Unavailable)
                        },
                        ctx,
                        "cpu:system",
                    ),
                    gpu_pct: Reading::unsupported(
                        "gpu:unselected",
                        "Selected-adapter process attribution is unavailable",
                    ),
                    power_w: Reading::unsupported(
                        "power:process",
                        "Measured process power is unavailable",
                    ),
                })
            })
            .collect();
        state.process_starts = starts;
        ProviderPoll {
            cpu: Some(cpu),
            memory: Some(memory),
            processes,
            ..ProviderPoll::default()
        }
    }
}

fn cpu_access() -> Result<(), ProviderError> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/stat").map_err(ProviderError::from)?;
        super::linux::system::parse_cpu(&text).ok_or(ProviderError::Invalid)?;
    }
    #[cfg(target_os = "windows")]
    {
        unsafe {
            ::windows::Win32::System::Threading::GetSystemTimes(
                Some(&mut ::windows::Win32::Foundation::FILETIME::default()),
                Some(&mut ::windows::Win32::Foundation::FILETIME::default()),
                Some(&mut ::windows::Win32::Foundation::FILETIME::default()),
            )
        }
        .map_err(|_| ProviderError::Unavailable)?;
    }
    Ok(())
}
fn memory_access() -> Result<(), ProviderError> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/meminfo").map_err(ProviderError::from)?;
        if !["MemTotal:", "MemAvailable:"]
            .iter()
            .all(|key| text.lines().any(|l| l.starts_with(key)))
        {
            return Err(ProviderError::Unavailable);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use ::windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe { GlobalMemoryStatusEx(&mut status) }.map_err(|_| ProviderError::Unavailable)?;
    }
    Ok(())
}
