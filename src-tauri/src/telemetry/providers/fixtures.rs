//! Deterministic providers for tests only. Compiled exclusively with the `test-fixtures` feature;
//! production binaries have no fixture-selection path or permission.

use crate::telemetry::model::{
    BatteryState, ByteCount, CpuMetrics, DiskMetrics, GpuMetrics, MemoryMetrics, NetworkMetrics,
    PowerMetrics, PowerPrimaryKind, Reading,
};
use crate::telemetry::providers::{PollContext, Provider, ProviderLoss, ProviderPoll};

pub const HOST_SCOPE: &str = "host:fixture";
pub const CPU_SCOPE: &str = "cpu:fixture-package";
pub const GPU_SCOPE: &str = "gpu:GPU-fixture";
pub const VOLUME_SCOPE: &str = "volume:fixture-root";
pub const NETWORK_SCOPE: &str = "network:fixture-aggregate";
pub const BATTERY_SCOPE: &str = "battery:fixture-BAT0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureScenario {
    Full,
    NoGpu,
    Denied,
}

pub struct FixtureProvider {
    scenario: FixtureScenario,
}

impl FixtureProvider {
    pub fn new(scenario: FixtureScenario) -> Self {
        Self { scenario }
    }
}

impl Provider for FixtureProvider {
    fn provider_id(&self) -> &'static str {
        "fixtures"
    }

    fn poll(&self, context: &PollContext<'_>) -> ProviderPoll {
        let mut poll = full_poll(context);
        match self.scenario {
            FixtureScenario::Full => {}
            FixtureScenario::NoGpu => {
                poll.gpu = None;
                poll.losses.push(ProviderLoss {
                    provider: "nvml".into(),
                    reason: "No supported GPU was discovered".into(),
                });
            }
            FixtureScenario::Denied => {
                let at = context.sampled_at_ms();
                let elapsed = context.elapsed_ms;
                poll.cpu = Some(CpuMetrics {
                    total_pct: denied(CPU_SCOPE, "CPU counters are not readable"),
                    user_pct: denied(CPU_SCOPE, "CPU counters are not readable"),
                    system_pct: denied(CPU_SCOPE, "CPU counters are not readable"),
                    temperature_c: valid(61.0, at, elapsed, CPU_SCOPE),
                });
                poll.losses.push(ProviderLoss {
                    provider: "sysinfo".into(),
                    reason: "CPU counters are not readable".into(),
                });
            }
        }
        poll
    }
}

fn valid<T>(value: T, at: i64, elapsed: u64, scope: &str) -> Reading<T> {
    Reading::valid(value, at, elapsed, scope)
}

fn denied<T>(scope: &str, reason: &str) -> Reading<T> {
    Reading::permission_denied(scope, reason)
}

fn full_poll(context: &PollContext<'_>) -> ProviderPoll {
    let at = context.sampled_at_ms();
    let elapsed = context.elapsed_ms;

    ProviderPoll {
        cpu: Some(CpuMetrics {
            total_pct: valid(42.5, at, elapsed, CPU_SCOPE),
            user_pct: valid(30.0, at, elapsed, CPU_SCOPE),
            system_pct: valid(12.5, at, elapsed, CPU_SCOPE),
            temperature_c: valid(61.0, at, elapsed, CPU_SCOPE),
        }),
        memory: Some(MemoryMetrics {
            total_bytes: valid(ByteCount(34_359_738_368), at, elapsed, HOST_SCOPE),
            used_bytes: valid(ByteCount(12_025_908_429), at, elapsed, HOST_SCOPE),
            available_bytes: valid(ByteCount(22_333_829_939), at, elapsed, HOST_SCOPE),
            cached_bytes: valid(ByteCount(4_294_967_296), at, elapsed, HOST_SCOPE),
            used_pct: valid(35.0, at, elapsed, HOST_SCOPE),
        }),
        gpu: Some(GpuMetrics {
            utilization_pct: valid(76.0, at, elapsed, GPU_SCOPE),
            vram_total_bytes: valid(ByteCount(12_884_901_888), at, elapsed, GPU_SCOPE),
            vram_used_bytes: valid(ByteCount(2_147_483_648), at, elapsed, GPU_SCOPE),
            vram_free_bytes: valid(ByteCount(9_663_676_416), at, elapsed, GPU_SCOPE),
            vram_reserved_bytes: valid(ByteCount(1_073_741_824), at, elapsed, GPU_SCOPE),
            temperature_c: valid(68.0, at, elapsed, GPU_SCOPE),
            power_w: valid(88.25, at, elapsed, GPU_SCOPE),
            fan_pct: valid(41.0, at, elapsed, GPU_SCOPE),
            power_limit_pct: valid(100.0, at, elapsed, GPU_SCOPE),
        }),
        disk: Some(DiskMetrics {
            total_bytes: valid(ByteCount(1_000_204_886_016), at, elapsed, VOLUME_SCOPE),
            free_bytes: valid(ByteCount(412_316_860_416), at, elapsed, VOLUME_SCOPE),
            read_bytes_per_second: valid(0.0, at, elapsed, VOLUME_SCOPE),
            write_bytes_per_second: valid(2_097_152.0, at, elapsed, VOLUME_SCOPE),
            written_session_bytes: valid(ByteCount(32_212_254_720), at, elapsed, VOLUME_SCOPE),
        }),
        network: Some(NetworkMetrics {
            receive_bytes_per_second: valid(0.0, at, elapsed, NETWORK_SCOPE),
            send_bytes_per_second: valid(0.0, at, elapsed, NETWORK_SCOPE),
            received_session_bytes: valid(ByteCount(0), at, elapsed, NETWORK_SCOPE),
            sent_session_bytes: valid(ByteCount(0), at, elapsed, NETWORK_SCOPE),
        }),
        power: Some(PowerMetrics {
            primary_kind: PowerPrimaryKind::BatteryPct,
            primary_value: valid(67.0, at, elapsed, BATTERY_SCOPE),
            watts: valid(9.75, at, elapsed, BATTERY_SCOPE),
            cpu_package_w: Reading::unavailable(CPU_SCOPE, "CPU package power is not available"),
            gpu_board_w: valid(88.25, at, elapsed, GPU_SCOPE),
            gpu_temperature_c: valid(68.0, at, elapsed, GPU_SCOPE),
            fan_pct: valid(41.0, at, elapsed, GPU_SCOPE),
            power_limit_pct: valid(100.0, at, elapsed, GPU_SCOPE),
            battery_charge_pct: valid(67.0, at, elapsed, BATTERY_SCOPE),
            battery_health_pct: valid(93.5, at, elapsed, BATTERY_SCOPE),
            battery_remaining_seconds: valid(9_000_u64, at, elapsed, BATTERY_SCOPE),
            battery_state: valid(BatteryState::Discharging, at, elapsed, BATTERY_SCOPE),
        }),
        losses: Vec::new(),
        ..ProviderPoll::default()
    }
}

/// Test-only connection point for T036/T038's serialized publication owner. Contract tests inject
/// coherent cached Views here, and inspect private bounds without any diagnostic IPC command.
/// Production builds cannot construct or install this probe.
#[derive(Default)]
pub struct LiveProbe {
    hooks: std::sync::Mutex<Option<LiveHooks>>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct LiveCounts {
    pub in_flight: usize,
    pub pending: usize,
    pub queued_controls: usize,
}
pub struct LiveHooks {
    pub publish: Box<dyn Fn(crate::telemetry::model::View) + Send + Sync>,
    pub counts: Box<dyn Fn() -> LiveCounts + Send + Sync>,
}
impl LiveProbe {
    pub fn install(&self, hooks: LiveHooks) {
        *self.hooks.lock().unwrap() = Some(hooks);
    }
    pub fn publish(&self, view: crate::telemetry::model::View) {
        let hooks = self.hooks.lock().unwrap();
        (hooks
            .as_ref()
            .expect("T036/T038 must attach the serialized publication owner to the test probe")
            .publish)(view);
    }
    pub fn counts(&self) -> LiveCounts {
        let hooks = self.hooks.lock().unwrap();
        (hooks
            .as_ref()
            .expect("T038 must attach the internal transport bounds probe")
            .counts)()
    }
}
