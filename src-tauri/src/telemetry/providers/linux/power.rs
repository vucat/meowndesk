//! Linux battery IDs are native power-supply device names; RAPL reads package domains only.
use super::system::read_u64;
use crate::telemetry::{model::*, normalize::empty_power, providers::*, rates::energy_delta};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};
pub fn battery(ctx: &PollContext<'_>) -> PowerMetrics {
    let mut paths: Vec<PathBuf> = fs::read_dir("/sys/class/power_supply")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    for path in paths {
        if fs::read_to_string(path.join("type"))
            .unwrap_or_default()
            .trim()
            != "Battery"
        {
            continue;
        }
        if fs::read_to_string(path.join("scope"))
            .ok()
            .is_some_and(|s| s.trim() == "Device")
        {
            continue;
        }
        if read_u64(path.join("present")).ok() == Some(0) {
            continue;
        }
        let Ok(charge) = read_u64(path.join("capacity")) else {
            continue;
        };
        if charge > 100 {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let scope = format!("battery:linux-{}", name.to_string_lossy());
        let mut power = empty_power(&scope);
        power.battery_charge_pct = observed(Ok(charge as f64), ctx, &scope);
        let status = fs::read_to_string(path.join("status")).unwrap_or_default();
        let state = match status.trim() {
            "Charging" => BatteryState::Charging,
            "Discharging" => BatteryState::Discharging,
            "Full" => BatteryState::Full,
            "Not charging" => BatteryState::OnAc,
            _ => BatteryState::Unknown,
        };
        power.battery_state = observed(Ok(state), ctx, &scope);
        let health = read_u64(path.join("energy_full"))
            .and_then(|full| {
                read_u64(path.join("energy_full_design")).and_then(|design| {
                    if design > 0 {
                        Ok(100.0 * full as f64 / design as f64)
                    } else {
                        Err(ProviderError::Invalid)
                    }
                })
            })
            .or_else(|_| {
                read_u64(path.join("charge_full")).and_then(|full| {
                    read_u64(path.join("charge_full_design")).and_then(|design| {
                        if design > 0 {
                            Ok(100.0 * full as f64 / design as f64)
                        } else {
                            Err(ProviderError::Invalid)
                        }
                    })
                })
            });
        power.battery_health_pct = observed(health, ctx, &scope);
        if state == BatteryState::Discharging {
            power.watts = observed(
                read_u64(path.join("power_now")).map(|v| v as f64 / 1_000_000.0),
                ctx,
                &scope,
            );
            // Only a provider-supplied remaining-time field is accepted, never derive a forecast.
            power.battery_remaining_seconds =
                observed(read_u64(path.join("time_to_empty_now")), ctx, &scope);
        }
        return power;
    }
    empty_power("battery:unavailable")
}
#[derive(Default)]
pub struct RaplTracker {
    last: BTreeMap<String, (u64, u64, u64)>,
}
impl RaplTracker {
    pub fn observe(
        &mut self,
        id: &str,
        energy: u64,
        range: u64,
        elapsed: u64,
        interval: u64,
    ) -> Option<f64> {
        let old = self.last.insert(id.into(), (energy, range, elapsed))?;
        let dt = elapsed.checked_sub(old.2)?;
        if dt == 0 || dt > interval.saturating_mul(2) || old.1 != range {
            return None;
        }
        let watts = energy_delta(old.0, energy, range)? as f64 / (dt as f64 * 1000.0);
        valid_nonnegative(&watts).then_some(watts)
    }
}
#[derive(Default)]
pub struct RaplProvider {
    tracker: Mutex<RaplTracker>,
}
impl Provider for RaplProvider {
    fn provider_id(&self) -> &'static str {
        "rapl"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        let mut paths: Vec<_> = fs::read_dir("/sys/class/powercap")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        paths.sort();
        let mut tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
        // One named package is a truthful CPU-package source; no parent/child or multi-socket sum.
        for path in paths {
            let name = fs::read_to_string(path.join("name")).unwrap_or_default();
            if !name.trim().starts_with("package-") {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let id = format!("cpu:rapl-{}", name.to_string_lossy());
            let mut power = empty_power(&id);
            let reading = read_u64(path.join("energy_uj")).and_then(|energy| {
                read_u64(path.join("max_energy_range_uj")).and_then(|range| {
                    tracker
                        .observe(&id, energy, range, ctx.elapsed_ms, ctx.interval_ms)
                        .ok_or(ProviderError::Unavailable)
                })
            });
            power.cpu_package_w = observed(reading, ctx, &id);
            return ProviderPoll {
                power: Some(power),
                ..ProviderPoll::default()
            };
        }
        ProviderPoll {
            power: Some(empty_power("cpu:rapl-unavailable")),
            ..ProviderPoll::default()
        }
    }
}
