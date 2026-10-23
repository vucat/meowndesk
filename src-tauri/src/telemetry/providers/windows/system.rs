//! GetSystemTimes includes idle in kernel time. LogicalDisk joins are exact system-volume joins.
#[derive(Clone, Copy, Debug)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}
pub fn cpu_split(old: CpuTimes, new: CpuTimes) -> Option<(f64, f64)> {
    let kernel = new.kernel.checked_sub(old.kernel)?;
    let user = new.user.checked_sub(old.user)?;
    let idle = new.idle.checked_sub(old.idle)?;
    let total = kernel.checked_add(user)?;
    let system = kernel.checked_sub(idle)?;
    if total == 0 {
        return None;
    }
    Some((
        100.0 * user as f64 / total as f64,
        100.0 * system as f64 / total as f64,
    ))
}
#[cfg(target_os = "windows")]
mod native {
    use super::super::pdh::{Query, text, wide};
    use super::{CpuTimes, cpu_split};
    use crate::telemetry::{
        model::*,
        normalize::{empty_cpu, empty_disk, empty_memory},
        providers::*,
        rates::{CounterBaseline, RateOutcome},
    };
    use ::windows::{
        Win32::{
            Foundation::FILETIME,
            Storage::FileSystem::{
                GetDiskFreeSpaceExW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
            },
            System::{SystemInformation::GetSystemDirectoryW, Threading::GetSystemTimes},
        },
        core::PCWSTR,
    };
    use std::sync::Mutex;
    fn time(v: FILETIME) -> u64 {
        (u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime)
    }
    fn cpu_times() -> Result<CpuTimes, ProviderError> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user))
                .map_err(|_| ProviderError::Unavailable)?;
        }
        Ok(CpuTimes {
            idle: time(idle),
            kernel: time(kernel),
            user: time(user),
        })
    }
    /// Standby/system cache in bytes; Windows reports it in pages, not as used RAM.
    fn cached_bytes() -> Result<u64, ProviderError> {
        use ::windows::Win32::System::ProcessStatus::{
            GetPerformanceInfo, PERFORMANCE_INFORMATION,
        };
        let info = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        let mut info = info;
        unsafe { GetPerformanceInfo(&mut info, info.cb) }
            .map_err(|_| ProviderError::Unavailable)?;
        u64::try_from(info.PageSize)
            .ok()
            .and_then(|page| u64::try_from(info.SystemCache).ok()?.checked_mul(page))
            .ok_or(ProviderError::Invalid)
    }
    #[derive(Clone, PartialEq)]
    struct Volume {
        path: String,
        id: String,
        instance: Option<String>,
    }
    fn system_volume() -> Result<Volume, ProviderError> {
        let mut directory = [0u16; 32768];
        let count = unsafe { GetSystemDirectoryW(Some(&mut directory)) };
        if count == 0 || count as usize >= directory.len() {
            return Err(ProviderError::Unavailable);
        }
        let mut path = [0u16; 32768];
        unsafe {
            GetVolumePathNameW(PCWSTR(directory.as_ptr()), &mut path)
                .map_err(|_| ProviderError::Unavailable)?;
        }
        let mut id = [0u16; 128];
        unsafe {
            GetVolumeNameForVolumeMountPointW(PCWSTR(path.as_ptr()), &mut id)
                .map_err(|_| ProviderError::Unavailable)?;
        }
        let path = text(&path);
        let instance =
            (path.len() == 3 && path.as_bytes()[1] == b':').then(|| path[..2].to_owned());
        Ok(Volume {
            path,
            id: text(&id),
            instance,
        })
    }
    #[derive(Default)]
    struct State {
        cpu: Option<(CpuTimes, u64)>,
        volume: Option<Volume>,
        query: Option<Query>,
        read: CounterBaseline,
        write: CounterBaseline,
        written: u64,
    }
    #[derive(Default)]
    pub struct WindowsSystemProvider {
        state: Mutex<State>,
    }
    impl Provider for WindowsSystemProvider {
        fn provider_id(&self) -> &'static str {
            "windows"
        }
        fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let mut cpu = empty_cpu("cpu:system");
            if let Ok(now) = cpu_times() {
                let split = state.cpu.and_then(|(old, t)| {
                    (ctx.elapsed_ms > t && ctx.elapsed_ms - t <= ctx.interval_ms * 2)
                        .then(|| cpu_split(old, now))
                        .flatten()
                });
                state.cpu = Some((now, ctx.elapsed_ms));
                cpu.user_pct = observed(
                    split.map(|v| v.0).ok_or(ProviderError::Unavailable),
                    ctx,
                    "cpu:system",
                );
                cpu.system_pct = observed(
                    split.map(|v| v.1).ok_or(ProviderError::Unavailable),
                    ctx,
                    "cpu:system",
                );
            } else {
                state.cpu = None;
            }
            cpu.temperature_c = Reading::unsupported(
                "cpu:system",
                "Package temperature is unavailable without a supported unprivileged sensor",
            );
            let volume = system_volume().ok();
            let mut memory = empty_memory("host:system");
            memory.cached_bytes = observed(cached_bytes().map(ByteCount), ctx, "host:system");
            if volume != state.volume {
                state.volume = volume;
                state.query = None;
                state.read.reset();
                state.write.reset();
            }
            let scope = state
                .volume
                .as_ref()
                .map(|v| format!("volume:{}", v.id))
                .unwrap_or_else(|| "volume:system".into());
            let mut disk = empty_disk(&scope);
            if let Some(volume) = state.volume.clone() {
                let path = wide(&volume.path);
                let (mut total, mut free) = (0, 0);
                let result = unsafe {
                    GetDiskFreeSpaceExW(
                        PCWSTR(path.as_ptr()),
                        None,
                        Some(&mut total),
                        Some(&mut free),
                    )
                };
                if result.is_ok() && total > 0 && free <= total {
                    disk.total_bytes = observed(Ok(ByteCount(total)), ctx, &scope);
                    disk.free_bytes = observed(Ok(ByteCount(free)), ctx, &scope);
                }
                if state.query.is_none()
                    && let Some(instance) = volume.instance
                {
                    state.query = Query::new(&[
                        format!("\\LogicalDisk({instance})\\Disk Read Bytes/sec"),
                        format!("\\LogicalDisk({instance})\\Disk Write Bytes/sec"),
                    ])
                    .ok();
                }
                let raw = state
                    .query
                    .as_mut()
                    .ok_or(ProviderError::Unavailable)
                    .and_then(|q| {
                        q.collect()?;
                        Ok((q.raw(0)?, q.raw(1)?))
                    });
                if let Ok((read, write)) = raw {
                    let r = state.read.observe(read, ctx.elapsed_ms, ctx.interval_ms);
                    let w = state.write.observe(write, ctx.elapsed_ms, ctx.interval_ms);
                    if let RateOutcome::Sample { delta, .. } = w
                        && w.per_second().is_some()
                    {
                        state.written = state.written.saturating_add(delta);
                    }
                    disk.read_bytes_per_second = observed(
                        r.per_second().ok_or(ProviderError::Unavailable),
                        ctx,
                        &scope,
                    );
                    disk.write_bytes_per_second = observed(
                        w.per_second().ok_or(ProviderError::Unavailable),
                        ctx,
                        &scope,
                    );
                    disk.written_session_bytes =
                        observed(Ok(ByteCount(state.written)), ctx, &scope);
                } else {
                    state.query = None;
                    state.read.reset();
                    state.write.reset();
                }
            }
            ProviderPoll {
                cpu: Some(cpu),
                memory: Some(memory),
                disk: Some(disk),
                ..ProviderPoll::default()
            }
        }
    }
}
#[cfg(target_os = "windows")]
pub use native::WindowsSystemProvider;
