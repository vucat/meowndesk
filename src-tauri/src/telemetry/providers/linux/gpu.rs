//! DRM/sysfs fallback preserves PCI identity. Shared system RAM is never dedicated VRAM.
use crate::telemetry::{
    model::*,
    normalize::{empty_gpu, power_limit_pct},
    providers::linux::system::{read_u64, temperature},
    providers::*,
};
use std::{fs, path::PathBuf};
#[derive(Clone, Debug)]
pub struct DrmAdapter {
    pub pci_id: String,
    pub vendor: GpuVendor,
    pub kind: GpuKind,
    pub(crate) path: PathBuf,
}
pub fn discover() -> Result<Vec<DrmAdapter>, ProviderError> {
    let mut adapters = Vec::new();
    for entry in fs::read_dir("/sys/class/drm")
        .map_err(ProviderError::from)?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name
            .strip_prefix("card")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let path = match fs::canonicalize(entry.path().join("device")) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let pci_id = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_string();
        if !pci_id.contains(':') {
            continue;
        }
        let vendor = fs::read_to_string(path.join("vendor")).unwrap_or_default();
        let vendor = match vendor.trim() {
            "0x1002" => GpuVendor::Amd,
            "0x8086" => GpuVendor::Intel,
            _ => continue,
        };
        // A readable, positive dedicated VRAM aperture is required; absent metadata remains
        // conservative rather than claiming host RAM as an adapter's dedicated memory.
        let kind = if vendor == GpuVendor::Amd {
            amd_kind(&pci_id)
        } else {
            GpuKind::Other
        };
        adapters.push(DrmAdapter {
            pci_id,
            vendor,
            kind,
            path,
        });
    }
    adapters.sort_by(|a, b| a.pci_id.cmp(&b.pci_id));
    adapters.dedup_by(|a, b| a.pci_id == b.pci_id);
    Ok(adapters)
}
pub struct DrmProvider {
    adapter: DrmAdapter,
}
impl DrmProvider {
    pub fn new(adapter: DrmAdapter) -> Self {
        Self { adapter }
    }
}
impl Provider for DrmProvider {
    fn provider_id(&self) -> &'static str {
        "drm"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        let adapter = &self.adapter;
        let scope = format!("gpu:pci:{}", adapter.pci_id);
        let mut gpu = empty_gpu(&scope);
        if adapter.vendor == GpuVendor::Amd {
            gpu.utilization_pct = observed(
                read_u64(adapter.path.join("gpu_busy_percent")).map(|v| v as f64),
                ctx,
                &scope,
            );
            if adapter.kind == GpuKind::Discrete {
                let total = read_u64(adapter.path.join("mem_info_vram_total"));
                let used = read_u64(adapter.path.join("mem_info_vram_used"));
                gpu.vram_total_bytes = observed(total.map(ByteCount), ctx, &scope);
                gpu.vram_used_bytes = observed(used.map(ByteCount), ctx, &scope);
                gpu.vram_free_bytes = observed(
                    total
                        .and_then(|t| {
                            used.and_then(|u| t.checked_sub(u).ok_or(ProviderError::Invalid))
                        })
                        .map(ByteCount),
                    ctx,
                    &scope,
                );
            }
        } else {
            gpu.utilization_pct = Reading::unsupported(
                &scope,
                "This Intel DRM provider has no trustworthy utilization counter",
            );
        }
        if adapter.kind != GpuKind::Discrete {
            gpu.vram_total_bytes = Reading::unsupported(
                &scope,
                if adapter.kind == GpuKind::Integrated {
                    "Integrated GPU uses shared system RAM; dedicated VRAM is unavailable"
                } else {
                    "A trustworthy dedicated-VRAM domain is not available"
                },
            );
            gpu.vram_used_bytes = gpu.vram_total_bytes.clone();
            gpu.vram_free_bytes = gpu.vram_total_bytes.clone();
        }
        gpu.vram_reserved_bytes =
            Reading::unsupported(&scope, "Disjoint reserved VRAM is not reported");
        for hw in fs::read_dir(adapter.path.join("hwmon"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let hw = hw.path();
            let driver = fs::read_to_string(hw.join("name")).unwrap_or_default();
            if !matches!(driver.trim(), "amdgpu" | "i915" | "xe") {
                continue;
            }
            gpu.temperature_c = observed(temperature(hw.join("temp1_input")), ctx, &scope);
            // APU power is SoC/package power, not a GPU-board measurement.
            if adapter.kind == GpuKind::Discrete {
                gpu.power_w = observed(
                    read_u64(hw.join("power1_average"))
                        .or_else(|_| read_u64(hw.join("power1_input")))
                        .map(|v| v as f64 / 1_000_000.0),
                    ctx,
                    &scope,
                );
            }
            gpu.fan_pct =
                Reading::unsupported(&scope, "Fan RPM or PWM is not a measured fan percentage");
            gpu.power_limit_pct = observed(
                read_u64(hw.join("power1_cap")).and_then(|cap| {
                    read_u64(hw.join("power1_cap_max")).and_then(|max| {
                        power_limit_pct(cap as f64, max as f64).ok_or(ProviderError::Invalid)
                    })
                }),
                ctx,
                &scope,
            );
            break;
        }
        ProviderPoll {
            gpu: Some(gpu),
            ..ProviderPoll::default()
        }
    }
}

/// KFD topology ties local-memory/CPU-node properties to a PCI function. Size thresholds and
/// vendor IDs cannot distinguish an APU's stolen system RAM from discrete VRAM.
fn amd_kind(pci: &str) -> GpuKind {
    let parts: Vec<_> = pci.split([':', '.']).collect();
    if parts.len() != 4 {
        return GpuKind::Other;
    }
    let numbers = parts
        .iter()
        .map(|s| u64::from_str_radix(s, 16))
        .collect::<Result<Vec<_>, _>>();
    let Ok(n) = numbers else {
        return GpuKind::Other;
    };
    let location = (n[1] << 8) | (n[2] << 3) | n[3];
    for entry in fs::read_dir("/sys/class/kfd/kfd/topology/nodes")
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(properties) = fs::read_to_string(entry.path().join("properties")) else {
            continue;
        };
        let get = |key: &str| {
            properties.lines().find_map(|line| {
                let mut f = line.split_whitespace();
                (f.next()? == key)
                    .then(|| f.next()?.parse::<u64>().ok())
                    .flatten()
            })
        };
        if get("domain") != Some(n[0])
            || get("location_id") != Some(location)
            || get("simd_count").unwrap_or(0) == 0
        {
            continue;
        }
        if get("cpu_cores_count").unwrap_or(0) > 0 {
            return GpuKind::Integrated;
        }
        if get("local_mem_size").unwrap_or(0) > 0 {
            return GpuKind::Discrete;
        }
    }
    GpuKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sysfs_fields_keep_units_and_never_call_shared_memory_dedicated_vram() {
        use crate::telemetry::clock::FakeClock;
        let path = std::env::temp_dir().join(format!(
            "meow-drm-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("hwmon/hwmon0")).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        for (name, value) in [
            ("gpu_busy_percent", "35"),
            ("mem_info_vram_total", "1000000000"),
            ("mem_info_vram_used", "200000000"),
            ("hwmon/hwmon0/name", "amdgpu"),
            ("hwmon/hwmon0/temp1_input", "55000"),
            ("hwmon/hwmon0/power1_average", "100000000"),
            ("hwmon/hwmon0/power1_cap", "200000000"),
            ("hwmon/hwmon0/power1_cap_max", "250000000"),
        ] {
            fs::write(path.join(name), value).unwrap();
        }
        let clock = FakeClock::new(0, 1);
        let ctx = PollContext {
            clock: &clock,
            elapsed_ms: 1000,
            interval_ms: 1000,
        };
        let adapter = DrmAdapter {
            pci_id: "0000:03:00.0".into(),
            vendor: GpuVendor::Amd,
            kind: GpuKind::Discrete,
            path,
        };
        let gpu = DrmProvider::new(adapter.clone()).poll(&ctx).gpu.unwrap();
        assert_eq!(gpu.utilization_pct.value, Some(35.0));
        assert_eq!(gpu.vram_free_bytes.value, Some(ByteCount(800000000)));
        assert_eq!(gpu.power_w.value, Some(100.0));
        assert_eq!(gpu.temperature_c.value, Some(55.0));
        assert_eq!(gpu.power_limit_pct.value, Some(80.0));
        assert_eq!(
            gpu.utilization_pct.scope_id.as_deref(),
            Some("gpu:pci:0000:03:00.0")
        );
        let mut integrated = adapter;
        integrated.kind = GpuKind::Integrated;
        let gpu = DrmProvider::new(integrated).poll(&ctx).gpu.unwrap();
        assert_eq!(gpu.vram_total_bytes.status, ReadingStatus::Unsupported);
        assert!(gpu.power_w.value.is_none());
    }
}
