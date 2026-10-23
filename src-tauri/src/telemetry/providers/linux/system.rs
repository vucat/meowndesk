//! Linux CPU splits, named package thermals, overlapping cache, and exact root-volume counters.
use crate::telemetry::{
    model::*,
    normalize::{empty_cpu, empty_disk, empty_memory},
    providers::*,
    rates::{CounterBaseline, RateOutcome},
};
use std::{fs, path::Path, sync::Mutex};
#[derive(Clone, Copy, Debug)]
pub struct CpuTimes {
    user: u64,
    system: u64,
    total: u64,
}
pub fn parse_cpu(text: &str) -> Option<CpuTimes> {
    let fields = text
        .lines()
        .find(|l| l.starts_with("cpu "))?
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if fields.len() < 8 {
        return None;
    }
    // guest/guest_nice are already included in user/nice, and must not be added again.
    Some(CpuTimes {
        user: fields[0].checked_add(fields[1])?,
        system: fields[2].checked_add(fields[5])?.checked_add(fields[6])?,
        total: fields[..8]
            .iter()
            .try_fold(0u64, |a, b| a.checked_add(*b))?,
    })
}
pub fn cpu_split(old: CpuTimes, new: CpuTimes) -> Option<(f64, f64)> {
    let total = new.total.checked_sub(old.total)?;
    if total == 0 {
        return None;
    }
    let user = 100.0 * new.user.checked_sub(old.user)? as f64 / total as f64;
    let system = 100.0 * new.system.checked_sub(old.system)? as f64 / total as f64;
    (valid_pct(&user) && valid_pct(&system) && user + system <= 100.0 + 1e-8)
        .then_some((user, system))
}
pub fn root_device(mountinfo: &str) -> Option<(u32, u32)> {
    let roots: Vec<_> = mountinfo
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            if f.get(4) != Some(&"/") {
                return None;
            }
            let (major, minor) = f.get(2)?.split_once(':')?;
            Some((major.parse().ok()?, minor.parse().ok()?))
        })
        .collect();
    // Ambiguous/stacked mounts are not sufficient evidence of an exact volume.
    (roots.len() == 1).then(|| roots[0])
}
pub fn disk_counters(text: &str, device: (u32, u32)) -> Option<(u64, u64)> {
    for line in text.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        if (f[0].parse().ok()?, f[1].parse().ok()?) == device {
            return Some((
                f[5].parse::<u64>().ok()?.checked_mul(512)?,
                f[9].parse::<u64>().ok()?.checked_mul(512)?,
            ));
        }
    }
    None
}
fn cached_bytes(text: &str) -> Option<u64> {
    let read = |key: &str| {
        text.lines().find_map(|l| {
            let (name, rest) = l.split_once(':')?;
            (name == key)
                .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    read("Cached")?
        .checked_add(read("Buffers")?)?
        .checked_mul(1024)
}
pub(crate) fn read_u64(path: impl AsRef<Path>) -> Result<u64, ProviderError> {
    fs::read_to_string(path)
        .map_err(ProviderError::from)?
        .trim()
        .parse()
        .map_err(|_| ProviderError::Invalid)
}
pub(crate) fn temperature(path: impl AsRef<Path>) -> Result<f64, ProviderError> {
    let raw = fs::read_to_string(path)
        .map_err(ProviderError::from)?
        .trim()
        .parse::<i64>()
        .map_err(|_| ProviderError::Invalid)?;
    let value = raw as f64 / 1000.0;
    valid_temperature(&value)
        .then_some(value)
        .ok_or(ProviderError::Invalid)
}
fn package_temperature() -> Result<f64, ProviderError> {
    package_temperature_in(Path::new("/sys/class/hwmon"))
}
fn package_temperature_in(root: &Path) -> Result<f64, ProviderError> {
    let mut denied = false;
    for dir in fs::read_dir(root).map_err(ProviderError::from)?.flatten() {
        let path = dir.path();
        let name = fs::read_to_string(path.join("name")).unwrap_or_default();
        if !matches!(name.trim(), "coretemp" | "k10temp" | "zenpower") {
            continue;
        }
        for file in fs::read_dir(&path).into_iter().flatten().flatten() {
            let filename = file.file_name().to_string_lossy().into_owned();
            if !filename.starts_with("temp") || !filename.ends_with("_label") {
                continue;
            }
            let label = fs::read_to_string(file.path()).unwrap_or_default();
            if !matches!(label.trim(), "Package id 0" | "Tdie" | "Tctl") {
                continue;
            }
            match temperature(path.join(filename.replace("_label", "_input"))) {
                Ok(value) => return Ok(value),
                Err(ProviderError::PermissionDenied) => denied = true,
                _ => {}
            }
        }
    }
    Err(if denied {
        ProviderError::PermissionDenied
    } else {
        ProviderError::Unsupported
    })
}
fn root_capacity() -> Result<(u64, u64), ProviderError> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: fixed nul-terminated root path and a correctly sized output buffer.
    if unsafe { libc::statvfs(c"/".as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stat = unsafe { stat.assume_init() };
    let total = stat
        .f_blocks
        .checked_mul(stat.f_frsize)
        .ok_or(ProviderError::Invalid)?;
    let free = stat
        .f_bfree
        .checked_mul(stat.f_frsize)
        .ok_or(ProviderError::Invalid)?;
    if total == 0 || free > total {
        return Err(ProviderError::Invalid);
    }
    Ok((total, free))
}
#[derive(Default)]
struct State {
    cpu: Option<(CpuTimes, u64)>,
    device: Option<(u32, u32)>,
    read: CounterBaseline,
    write: CounterBaseline,
    written: u64,
}
#[derive(Default)]
pub struct LinuxSystemProvider {
    state: Mutex<State>,
}
impl Provider for LinuxSystemProvider {
    fn provider_id(&self) -> &'static str {
        "linux"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut cpu = empty_cpu("cpu:system");
        match fs::read_to_string("/proc/stat")
            .map_err(ProviderError::from)
            .and_then(|s| parse_cpu(&s).ok_or(ProviderError::Invalid))
        {
            Ok(now) => {
                let split = state.cpu.and_then(|(old, t)| {
                    (ctx.elapsed_ms > t && ctx.elapsed_ms - t <= 2 * ctx.interval_ms)
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
            }
            Err(e) => {
                state.cpu = None;
                cpu.user_pct = e.reading("cpu:system", "CPU counters are not readable");
                cpu.system_pct = cpu.user_pct.clone();
            }
        }
        cpu.temperature_c = observed(package_temperature(), ctx, "cpu:system");
        let mut memory = empty_memory("host:system");
        memory.cached_bytes = observed(
            fs::read_to_string("/proc/meminfo")
                .map_err(ProviderError::from)
                .and_then(|s| {
                    cached_bytes(&s)
                        .map(ByteCount)
                        .ok_or(ProviderError::Invalid)
                }),
            ctx,
            "host:system",
        );
        let device = fs::read_to_string("/proc/self/mountinfo")
            .ok()
            .and_then(|s| root_device(&s));
        if device != state.device {
            state.read.reset();
            state.write.reset();
            state.device = device;
        }
        let scope = device
            .map(|(a, b)| format!("volume:linux-{a}:{b}"))
            .unwrap_or_else(|| "volume:linux-root".into());
        let mut disk = empty_disk(&scope);
        let capacity = root_capacity();
        disk.total_bytes = observed(capacity.map(|v| ByteCount(v.0)), ctx, &scope);
        disk.free_bytes = observed(capacity.map(|v| ByteCount(v.1)), ctx, &scope);
        let counters = device.ok_or(ProviderError::Unavailable).and_then(|d| {
            fs::read_to_string("/proc/diskstats")
                .map_err(ProviderError::from)
                .and_then(|s| disk_counters(&s, d).ok_or(ProviderError::Unavailable))
        });
        if let Ok((read, write)) = counters {
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
            disk.written_session_bytes = observed(Ok(ByteCount(state.written)), ctx, &scope);
        } else if let Err(err) = counters {
            state.read.reset();
            state.write.reset();
            disk.read_bytes_per_second =
                err.reading(&scope, "Exact root-volume counters are not readable");
            disk.write_bytes_per_second = disk.read_bytes_per_second.clone();
        }
        ProviderPoll {
            cpu: Some(cpu),
            memory: Some(memory),
            disk: Some(disk),
            ..ProviderPoll::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_temperature_uses_named_cpu_labels_not_the_first_hwmon_sensor() {
        let root = std::env::temp_dir().join(format!(
            "meow-hwmon-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        for (folder, driver, label, temp) in [
            ("hwmon0", "motherboard", "CPU", "99000"),
            ("hwmon1", "coretemp", "Core 0", "45000"),
            ("hwmon2", "coretemp", "Package id 0", "61000"),
        ] {
            let p = root.join(folder);
            fs::create_dir_all(&p).unwrap();
            for (file, value) in [
                ("name", driver),
                ("temp1_label", label),
                ("temp1_input", temp),
            ] {
                fs::write(p.join(file), value).unwrap();
            }
        }
        assert_eq!(package_temperature_in(&root), Ok(61.0));
        fs::remove_dir_all(root.join("hwmon2")).unwrap();
        assert_eq!(
            package_temperature_in(&root),
            Err(ProviderError::Unsupported)
        );
    }
}
