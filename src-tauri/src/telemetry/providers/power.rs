//! Battery readings never claim whole-machine power. RAPL is an independent CPU-package scope.
use crate::telemetry::{model::*, normalize::empty_power, providers::*};
use battery::units::{power::watt, ratio::percent, time::second};

pub struct BatteryProvider;
impl Default for BatteryProvider {
    fn default() -> Self {
        Self
    }
}
impl Provider for BatteryProvider {
    fn provider_id(&self) -> &'static str {
        "battery"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        #[cfg(target_os = "linux")]
        let power = super::linux::power::battery(ctx);
        #[cfg(not(target_os = "linux"))]
        let power = read_battery(ctx);
        ProviderPoll {
            power: Some(power),
            ..ProviderPoll::default()
        }
    }
}
/// battery 0.7's Manager uses Rc and must stay on the collection thread. Cache it in thread-local
/// storage rather than marking a non-Send native manager Send or moving it between workers.
#[derive(Default)]
struct BatteryOwner {
    manager: Option<battery::Manager>,
}
thread_local! {static BATTERIES: std::cell::RefCell<BatteryOwner> = std::cell::RefCell::new(BatteryOwner::default());}
pub fn release_battery_owner() {
    BATTERIES.with(|owner| owner.borrow_mut().manager = None);
}
#[allow(dead_code)]
fn read_battery(ctx: &PollContext<'_>) -> PowerMetrics {
    BATTERIES.with(|owner| {
        let mut owner = owner.borrow_mut();
        if owner.manager.is_none() {
            owner.manager = battery::Manager::new().ok();
        }
        let mut power = empty_power("battery:unavailable");
        let Some(manager) = owner.manager.as_ref() else {
            return power;
        };
        let Ok(batteries) = manager.batteries() else {
            return power;
        };
        let mut readable: Vec<_> = batteries
            .flatten()
            .filter_map(|b| {
                let id = b.serial_number()?.trim();
                if id.is_empty() {
                    return None;
                }
                let id = crate::telemetry::bounds::bounded_label(id, 100);
                Some((id, b))
            })
            .collect();
        readable.sort_by(|a, b| a.0.cmp(&b.0));
        let Some((id, b)) = readable
            .into_iter()
            .find(|(_, b)| b.state_of_charge().get::<percent>().is_finite())
        else {
            return power;
        };
        let scope = format!("battery:{id}");
        power = empty_power(&scope);
        power.battery_charge_pct =
            observed(Ok(b.state_of_charge().get::<percent>() as f64), ctx, &scope);
        power.battery_health_pct =
            observed(Ok(b.state_of_health().get::<percent>() as f64), ctx, &scope);
        let state = match b.state() {
            battery::State::Charging => BatteryState::Charging,
            battery::State::Discharging => BatteryState::Discharging,
            battery::State::Full => BatteryState::Full,
            battery::State::Empty => BatteryState::Unknown,
            _ => BatteryState::Unknown,
        };
        power.battery_state = observed(Ok(state), ctx, &scope);
        if state == BatteryState::Discharging {
            power.watts = observed(Ok(b.energy_rate().get::<watt>() as f64), ctx, &scope);
            let remaining = b
                .time_to_empty()
                .map(|v| v.get::<second>() as f64)
                .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 9_007_199_254_740_991.0)
                .map(|v| v as u64)
                .ok_or(ProviderError::Unavailable);
            power.battery_remaining_seconds = observed(remaining, ctx, &scope);
        }
        power
    })
}
