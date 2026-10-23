//! Windows GPU engine accounting and native DXGI/PDH fallback. LUID is never persisted.
use crate::telemetry::model::valid_pct;
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct EngineCounter {
    pub instance: String,
    pub luid: u64,
    pub physical: u32,
    pub engine: u32,
    pub pid: u32,
    pub pct: f64,
}
pub fn busiest_engine(luid: u64, values: &[EngineCounter]) -> Option<f64> {
    let contributions = contributions(luid, values)?;
    let mut engines: BTreeMap<_, f64> = BTreeMap::new();
    for ((physical, engine, _), value) in contributions {
        *engines.entry((physical, engine)).or_default() += value;
    }
    if engines.is_empty()
        || engines
            .values()
            .any(|v| !v.is_finite() || *v > 100.0 + 1e-8)
    {
        return None;
    }
    engines.into_values().reduce(f64::max).map(|v| v.min(100.0))
}

/// Sums each PID's contributions per physical engine, then reports its busiest engine.
pub fn per_process_utilization(luid: u64, values: &[EngineCounter]) -> Option<BTreeMap<u32, f64>> {
    let contributions = contributions(luid, values)?;
    let mut engines: BTreeMap<(u32, u32, u32), f64> = BTreeMap::new();
    for ((physical, engine, pid), value) in contributions {
        *engines.entry((physical, engine, pid)).or_default() += value;
    }
    let mut per_pid: BTreeMap<u32, f64> = BTreeMap::new();
    for ((_, _, pid), value) in engines {
        if !value.is_finite() || value > 100.0 + 1e-8 {
            return None;
        }
        per_pid
            .entry(pid)
            .and_modify(|current| *current = current.max(value))
            .or_insert(value);
    }
    Some(
        per_pid
            .into_iter()
            .map(|(pid, value)| (pid, value.min(100.0)))
            .collect(),
    )
}

fn contributions(luid: u64, values: &[EngineCounter]) -> Option<BTreeMap<(u32, u32, u32), f64>> {
    let mut instances = BTreeMap::new();
    let mut contributions = BTreeMap::new();
    for value in values.iter().filter(|value| value.luid == luid) {
        if !valid_pct(&value.pct) {
            return None;
        }
        if let Some(previous) = instances.insert(value.instance.clone(), value.pct) {
            if (previous - value.pct).abs() > 1e-8 {
                return None;
            }
            continue;
        }
        let key = (value.physical, value.engine, value.pid);
        if let Some(previous) = contributions.insert(key, value.pct)
            && (previous - value.pct).abs() > 1e-8
        {
            return None;
        }
    }
    Some(contributions)
}
pub fn parse_engine(instance: &str, pct: f64) -> Option<EngineCounter> {
    let fields: Vec<_> = instance.split('_').collect();
    let get = |key: &str| {
        fields
            .iter()
            .position(|v| *v == key)
            .and_then(|i| fields.get(i + 1))
            .copied()
    };
    let position = fields.iter().position(|v| *v == "luid")?;
    let high = u32::from_str_radix(fields.get(position + 1)?.trim_start_matches("0x"), 16).ok()?;
    let low = u32::from_str_radix(fields.get(position + 2)?.trim_start_matches("0x"), 16).ok()?;
    Some(EngineCounter {
        instance: instance.into(),
        luid: (u64::from(high) << 32) | u64::from(low),
        physical: get("phys")?.parse().ok()?,
        engine: get("eng")?.parse().ok()?,
        pid: get("pid")?.parse().ok()?,
        pct,
    })
}
#[cfg(target_os = "windows")]
mod native {
    use super::super::pdh::{Query, text, wide};
    use super::{busiest_engine, parse_engine, per_process_utilization};
    use crate::telemetry::{bounds, model::*, normalize::empty_gpu, providers::*};
    use ::windows::{
        Wdk::Graphics::Direct3D::*,
        Win32::{
            Devices::DeviceAndDriverInstallation::{
                CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_SUCCESS,
            },
            Foundation::LUID,
            Graphics::Dxgi::*,
        },
        core::{PCWSTR, PWSTR},
    };
    use std::sync::Mutex;
    #[derive(Clone, Debug)]
    pub struct Adapter {
        pub descriptor: GpuDescriptor,
        pub runtime_luid: u64,
        pub dedicated_bytes: u64,
        pub shared_bytes: u64,
    }
    fn pnp_id(luid: LUID) -> Option<String> {
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: luid,
            ..Default::default()
        };
        if unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }.0 < 0 {
            return None;
        }
        struct Handle(u32);
        impl Drop for Handle {
            fn drop(&mut self) {
                unsafe {
                    let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: self.0 });
                }
            }
        }
        let handle = Handle(open.hAdapter);
        // A linked display adapter can span multiple physical devices. Reject ambiguous memory
        // capacity joins rather than assigning an aggregate to physical adapter zero's PnP ID.
        let mut count = D3DKMT_PHYSICAL_ADAPTER_COUNT::default();
        let mut count_query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: handle.0,
            Type: KMTQAITYPE_PHYSICALADAPTERCOUNT,
            pPrivateDriverData: (&mut count as *mut D3DKMT_PHYSICAL_ADAPTER_COUNT).cast(),
            PrivateDriverDataSize: std::mem::size_of_val(&count) as u32,
        };
        if unsafe { D3DKMTQueryAdapterInfo(&mut count_query) }.0 < 0 || count.Count != 1 {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let mut key = D3DKMT_QUERY_PHYSICAL_ADAPTER_PNP_KEY {
            PhysicalAdapterIndex: 0,
            PnPKeyType: D3DKMT_PNP_KEY_HARDWARE,
            pDest: PWSTR(buffer.as_mut_ptr()),
            pCchDest: &mut size,
        };
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: handle.0,
            Type: KMTQAITYPE_PHYSICALADAPTERPNPKEY,
            pPrivateDriverData: (&mut key as *mut D3DKMT_QUERY_PHYSICAL_ADAPTER_PNP_KEY).cast(),
            PrivateDriverDataSize: std::mem::size_of_val(&key) as u32,
        };
        if unsafe { D3DKMTQueryAdapterInfo(&mut query) }.0 < 0 {
            return None;
        }
        let key = text(&buffer);
        let lower = key.to_ascii_lowercase();
        let prefix = "\\registry\\machine\\system\\currentcontrolset\\enum\\";
        let id = if lower.starts_with(prefix) {
            key[prefix.len()..].to_owned()
        } else {
            key
        };
        if id.is_empty() || id.len() > 124 {
            return None;
        }
        let encoded = wide(&id);
        let mut devnode = 0;
        if unsafe {
            CM_Locate_DevNodeW(
                &mut devnode,
                PCWSTR(encoded.as_ptr()),
                CM_LOCATE_DEVNODE_NORMAL,
            )
        } != CR_SUCCESS
        {
            return None;
        }
        Some(id)
    }
    pub fn discover() -> Result<Vec<Adapter>, ProviderError> {
        let factory: IDXGIFactory1 =
            unsafe { CreateDXGIFactory1() }.map_err(|_| ProviderError::Unavailable)?;
        let mut adapters = Vec::new();
        let mut index = 0;
        loop {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(_) => return Err(ProviderError::Unavailable),
            };
            index += 1;
            let desc = unsafe { adapter.GetDesc1() }.map_err(|_| ProviderError::Unavailable)?;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let Some(id) = pnp_id(desc.AdapterLuid) else {
                continue;
            };
            let vendor = match desc.VendorId {
                0x10de => GpuVendor::Nvidia,
                0x1002 => GpuVendor::Amd,
                0x8086 => GpuVendor::Intel,
                _ => GpuVendor::Other,
            };
            adapters.push(Adapter {
                descriptor: GpuDescriptor {
                    id,
                    name: bounds::bounded_label(&text(&desc.Description), 128),
                    vendor,
                    kind: if desc.DedicatedVideoMemory == 0 {
                        GpuKind::Integrated
                    } else {
                        GpuKind::Discrete
                    },
                    available: true,
                },
                runtime_luid: ((desc.AdapterLuid.HighPart as u32 as u64) << 32)
                    | desc.AdapterLuid.LowPart as u64,
                dedicated_bytes: desc.DedicatedVideoMemory as u64,
                shared_bytes: desc.SharedSystemMemory as u64,
            });
        }
        adapters.sort_by(|a, b| a.descriptor.id.cmp(&b.descriptor.id));
        Ok(adapters)
    }
    #[derive(Default)]
    struct State {
        query: Option<Query>,
        adapter: Option<Adapter>,
        discovered_at: Option<u64>,
        last_poll: Option<u64>,
    }
    pub struct WindowsGpuProvider {
        selected_pnp_id: String,
        state: Mutex<State>,
    }
    impl WindowsGpuProvider {
        pub fn new(selected_pnp_id: String) -> Self {
            Self {
                selected_pnp_id,
                state: Mutex::new(State::default()),
            }
        }
        pub fn select(&mut self, pnp_id: String) {
            self.selected_pnp_id = pnp_id;
            *self.state.get_mut().unwrap_or_else(|e| e.into_inner()) = State::default();
        }
    }
    impl Provider for WindowsGpuProvider {
        fn provider_id(&self) -> &'static str {
            "dxgi-pdh"
        }
        fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
            let scope = format!("gpu:{}", self.selected_pnp_id);
            let mut gpu = empty_gpu(&scope);
            let mut process_gpu = Vec::new();
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.adapter.is_none()
                || state
                    .discovered_at
                    .is_none_or(|t| ctx.elapsed_ms.saturating_sub(t) >= 30_000)
            {
                let adapter = discover().ok().and_then(|a| {
                    a.into_iter()
                        .find(|a| a.descriptor.id.eq_ignore_ascii_case(&self.selected_pnp_id))
                });
                if state.adapter.as_ref().map(|a| a.runtime_luid)
                    != adapter.as_ref().map(|a| a.runtime_luid)
                {
                    state.query = None;
                }
                state.adapter = adapter;
                state.discovered_at = Some(ctx.elapsed_ms);
            }
            if state.last_poll.is_some_and(|t| {
                ctx.elapsed_ms <= t || ctx.elapsed_ms - t > ctx.interval_ms.saturating_mul(2)
            }) {
                state.query = None;
            }
            state.last_poll = Some(ctx.elapsed_ms);
            if let Some(adapter) = state.adapter.clone() {
                if state.query.is_none() {
                    state.query = Query::new(&[
                        r"\GPU Engine(*)\Utilization Percentage".into(),
                        r"\GPU Adapter Memory(*)\Dedicated Usage".into(),
                    ])
                    .ok();
                }
                if let Some(query) = state.query.as_mut() {
                    if query.collect().is_ok() {
                        let parsed: Vec<_> = query
                            .array(0)
                            .ok()
                            .map(|values| {
                                values
                                    .iter()
                                    .filter_map(|(name, pct)| parse_engine(name, *pct))
                                    .collect()
                            })
                            .unwrap_or_default();
                        // Unknown matching instances must not silently reduce a total.
                        let engines = busiest_engine(adapter.runtime_luid, &parsed);
                        gpu.utilization_pct =
                            observed(engines.ok_or(ProviderError::Unavailable), ctx, &scope);
                        process_gpu = per_process_utilization(adapter.runtime_luid, &parsed)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|(pid, pct)| ProcessGpuAttribution {
                                pid,
                                reading: observed(Ok(pct), ctx, &scope),
                            })
                            .collect();
                        if adapter.dedicated_bytes > 0 {
                            gpu.vram_total_bytes =
                                observed(Ok(ByteCount(adapter.dedicated_bytes)), ctx, &scope);
                            let key = format!(
                                "luid_0x{:08x}_0x{:08x}_phys_0",
                                (adapter.runtime_luid >> 32) as u32,
                                adapter.runtime_luid as u32
                            );
                            let used = query.array(1).ok().and_then(|values| {
                                let selected: Vec<_> = values
                                    .into_iter()
                                    .filter(|(name, _)| name.eq_ignore_ascii_case(&key))
                                    .collect();
                                (selected.len() == 1)
                                    .then(|| selected[0].1)
                                    .filter(|v| {
                                        v.is_finite()
                                            && *v >= 0.0
                                            && *v <= adapter.dedicated_bytes as f64
                                    })
                                    .map(|v| v as u64)
                            });
                            gpu.vram_used_bytes = observed(
                                used.map(ByteCount).ok_or(ProviderError::Unavailable),
                                ctx,
                                &scope,
                            );
                            gpu.vram_free_bytes = observed(
                                used.and_then(|used| adapter.dedicated_bytes.checked_sub(used))
                                    .map(ByteCount)
                                    .ok_or(ProviderError::Unavailable),
                                ctx,
                                &scope,
                            );
                        } else {
                            gpu.vram_total_bytes = Reading::unsupported(
                                &scope,
                                "Integrated GPU uses shared system RAM; dedicated VRAM is unavailable",
                            );
                            gpu.vram_used_bytes = gpu.vram_total_bytes.clone();
                            gpu.vram_free_bytes = gpu.vram_total_bytes.clone();
                        }
                    } else {
                        state.query = None;
                        state.adapter = None;
                    }
                }
            }
            gpu.vram_reserved_bytes =
                Reading::unsupported(&scope, "Disjoint reserved VRAM is not reported");
            gpu.temperature_c =
                Reading::unsupported(&scope, "DXGI/PDH does not supply GPU thermals");
            gpu.power_w =
                Reading::unsupported(&scope, "DXGI/PDH does not supply measured board watts");
            gpu.fan_pct = gpu.power_w.clone();
            gpu.power_limit_pct = gpu.power_w.clone();
            ProviderPoll {
                gpu: Some(gpu),
                process_gpu,
                ..ProviderPoll::default()
            }
        }
    }
}
#[cfg(target_os = "windows")]
pub use native::{Adapter, WindowsGpuProvider, discover};
