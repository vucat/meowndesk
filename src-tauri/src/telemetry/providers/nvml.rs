//! Dynamically loaded NVML stays owned for the provider lifetime. No CLI or CUDA dependency.
use crate::telemetry::{
    model::*,
    normalize::{empty_gpu, power_limit_pct},
    providers::*,
};
use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor, error::NvmlError};
use std::{collections::BTreeMap, ffi::OsStr, sync::Mutex};

/// Keep the native loader first. WSL mounts are fixed trusted paths, never IPC input.
/// WSL may find a native Linux library that loads but cannot initialize its driver.
/// Try both mounted libraries after any default initialization failure; preserve meaningful
/// driver/permission errors when subsequent paths are merely missing.
fn initialize_with<T>(
    mut load: impl FnMut(Option<&'static OsStr>) -> Result<T, NvmlError>,
) -> Result<T, NvmlError> {
    let original = match load(None) {
        Ok(owner) => return Ok(owner),
        Err(error) => error,
    };
    #[cfg(target_os = "linux")]
    {
        let mut failure = original;
        for path in [
            "/usr/lib/wsl/lib/libnvidia-ml.so.1",
            "/usr/lib/wsl/lib/libnvidia-ml.so",
        ] {
            match load(Some(OsStr::new(path))) {
                Ok(owner) => return Ok(owner),
                Err(NvmlError::LibloadingError(_)) => {}
                Err(error) => failure = error,
            }
        }
        Err(failure)
    }
    #[cfg(not(target_os = "linux"))]
    Err(original)
}

pub struct NvmlProvider {
    nvml: Nvml,
    selected_uuid: String,
    process_seen: Mutex<u64>,
}
fn error(e: NvmlError) -> ProviderError {
    match e {
        NvmlError::NoPermission => ProviderError::PermissionDenied,
        NvmlError::NotSupported | NvmlError::FunctionNotFound => ProviderError::Unsupported,
        _ => ProviderError::Unavailable,
    }
}
impl NvmlProvider {
    pub fn new(selected_uuid: String) -> Result<Self, ProviderError> {
        Ok(Self {
            nvml: initialize_with(|path| match path {
                None => Nvml::init(),
                Some(path) => Nvml::builder().lib_path(path).init(),
            })
            .map_err(error)?,
            selected_uuid,
            process_seen: Mutex::new(0),
        })
    }
    pub fn discover(&self) -> Vec<GpuDescriptor> {
        (0..self.nvml.device_count().unwrap_or(0))
            .filter_map(|index| {
                let device = self.nvml.device_by_index(index).ok()?;
                Some(GpuDescriptor {
                    id: device.uuid().ok()?,
                    name: crate::telemetry::bounds::bounded_label(&device.name().ok()?, 128),
                    vendor: GpuVendor::Nvidia,
                    kind: GpuKind::Discrete,
                    available: true,
                })
            })
            .collect()
    }
    pub fn select(&mut self, uuid: String) {
        self.selected_uuid = uuid;
        *self
            .process_seen
            .get_mut()
            .unwrap_or_else(|error| error.into_inner()) = 0;
    }
}
impl Provider for NvmlProvider {
    fn provider_id(&self) -> &'static str {
        "nvml"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        let scope = format!("gpu:{}", self.selected_uuid);
        let mut gpu = empty_gpu(&scope);
        let mut process_gpu = Vec::new();
        match self.nvml.device_by_uuid(self.selected_uuid.as_str()) {
            Ok(device) => {
                gpu.utilization_pct = observed(
                    device
                        .utilization_rates()
                        .map(|v| v.gpu as f64)
                        .map_err(error),
                    ctx,
                    &scope,
                );
                let memory = device.memory_info().map_err(error);
                gpu.vram_total_bytes = observed(
                    memory.as_ref().map(|v| ByteCount(v.total)).map_err(|e| *e),
                    ctx,
                    &scope,
                );
                gpu.vram_used_bytes = observed(
                    memory.as_ref().map(|v| ByteCount(v.used)).map_err(|e| *e),
                    ctx,
                    &scope,
                );
                gpu.vram_free_bytes = observed(
                    memory.as_ref().map(|v| ByteCount(v.free)).map_err(|e| *e),
                    ctx,
                    &scope,
                );
                gpu.vram_reserved_bytes =
                    Reading::unsupported(&scope, "NVML v1 does not expose disjoint reserved VRAM");
                gpu.temperature_c = observed(
                    device
                        .temperature(TemperatureSensor::Gpu)
                        .map(|v| v as f64)
                        .map_err(error),
                    ctx,
                    &scope,
                );
                gpu.fan_pct = observed(
                    device.fan_speed(0).map(|v| v as f64).map_err(error),
                    ctx,
                    &scope,
                );
                gpu.power_w = observed(
                    device
                        .power_usage()
                        .map(|v| v as f64 / 1000.0)
                        .map_err(error),
                    ctx,
                    &scope,
                );
                let limit = device
                    .power_management_limit()
                    .map_err(error)
                    .and_then(|configured| {
                        device
                            .power_management_limit_constraints()
                            .map_err(error)
                            .and_then(|max| {
                                power_limit_pct(configured as f64, max.max_limit as f64)
                                    .ok_or(ProviderError::Invalid)
                            })
                    });
                gpu.power_limit_pct = observed(limit, ctx, &scope);
                let mut seen = self
                    .process_seen
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Ok(samples) = device.process_utilization_stats(Some(*seen)) {
                    let mut by_pid: BTreeMap<u32, f64> = BTreeMap::new();
                    let mut latest = *seen;
                    for sample in samples {
                        latest = latest.max(sample.timestamp);
                        let pct =
                            f64::from(sample.sm_util.max(sample.enc_util).max(sample.dec_util));
                        if sample.pid == 0 || !valid_pct(&pct) {
                            continue;
                        }
                        by_pid
                            .entry(sample.pid)
                            .and_modify(|value| *value = value.max(pct))
                            .or_insert(pct);
                    }
                    *seen = latest;
                    drop(seen);
                    process_gpu = by_pid
                        .into_iter()
                        .map(|(pid, pct)| ProcessGpuAttribution {
                            pid,
                            reading: observed(Ok(pct), ctx, &scope),
                        })
                        .collect();
                }
            }
            Err(e) => {
                let e = error(e);
                gpu.utilization_pct =
                    e.reading(&scope, "The selected NVML adapter is not readable");
                gpu.vram_total_bytes =
                    e.reading(&scope, "The selected NVML adapter is not readable");
                gpu.vram_used_bytes = gpu.vram_total_bytes.clone();
                gpu.vram_free_bytes = gpu.vram_total_bytes.clone();
                gpu.vram_reserved_bytes = gpu.vram_total_bytes.clone();
                gpu.temperature_c = gpu.utilization_pct.clone();
                gpu.fan_pct = gpu.utilization_pct.clone();
                gpu.power_w = gpu.utilization_pct.clone();
                gpu.power_limit_pct = gpu.utilization_pct.clone();
            }
        }
        ProviderPoll {
            gpu: Some(gpu),
            process_gpu,
            ..ProviderPoll::default()
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/nvml_loading.rs"]
mod loading_tests;
