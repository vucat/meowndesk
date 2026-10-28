//! Version-1 wire DTO catalog plus boundary validation shared with the TypeScript codec.

use std::fmt;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::telemetry::bounds;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_JSON_SAFE_INT: u64 = 9_007_199_254_740_991;
pub const MAX_SCOPES: usize = bounds::MAX_SCOPES;
pub const MAX_CAPABILITIES: usize = bounds::MAX_CAPABILITIES;
pub const MAX_PROCESS_MEMORY_ENTRIES: usize = 8;
pub const MAX_PROCESS_RANK_ENTRIES: usize = 5;
pub const MAX_BREAKDOWN_SEGMENTS: usize = 8;
pub const CHART_COLUMNS: usize = 60;
pub const MAX_GPUS: usize = 16;

pub const CHART_SERIES_IDS: [&str; 6] = [
    "cpuPct",
    "ramUsedPct",
    "gpuPct",
    "diskBytesPerSecond",
    "networkBytesPerSecond",
    "powerW",
];

pub const METRIC_CATALOG: &[&str] = &[
    "cpu.totalPct",
    "cpu.userPct",
    "cpu.systemPct",
    "cpu.temperatureC",
    "memory.totalBytes",
    "memory.usedBytes",
    "memory.availableBytes",
    "memory.cachedBytes",
    "memory.usedPct",
    "gpu.utilizationPct",
    "gpu.vramTotalBytes",
    "gpu.vramUsedBytes",
    "gpu.vramFreeBytes",
    "gpu.vramReservedBytes",
    "gpu.temperatureC",
    "gpu.powerW",
    "gpu.fanPct",
    "gpu.powerLimitPct",
    "disk.totalBytes",
    "disk.freeBytes",
    "disk.readBytesPerSecond",
    "disk.writeBytesPerSecond",
    "disk.writtenSessionBytes",
    "network.receiveBytesPerSecond",
    "network.sendBytesPerSecond",
    "network.receivedSessionBytes",
    "network.sentSessionBytes",
    "power.primaryValue",
    "power.watts",
    "power.cpuPackageW",
    "power.gpuBoardW",
    "power.gpuTemperatureC",
    "power.fanPct",
    "power.powerLimitPct",
    "power.batteryChargePct",
    "power.batteryHealthPct",
    "power.batteryRemainingSeconds",
    "power.batteryState",
    "battery.chargePct",
    "battery.healthPct",
    "battery.remainingSeconds",
    "battery.state",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError(String);

impl ModelError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

pub type ModelResult<T> = Result<T, ModelError>;

fn fail<T>(message: impl Into<String>) -> ModelResult<T> {
    Err(ModelError::new(message))
}

pub fn is_json_safe(value: u64) -> bool {
    value <= MAX_JSON_SAFE_INT
}

pub fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let expected = [8, 4, 4, 4, 12];
    parts.len() == expected.len()
        && parts
            .iter()
            .zip(expected)
            .all(|(part, len)| part.len() == len && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn valid_pct(value: &f64) -> bool {
    value.is_finite() && (0.0..=100.0).contains(value)
}

pub fn valid_temperature(value: &f64) -> bool {
    value.is_finite() && (-273.15..=1_000.0).contains(value)
}

pub fn valid_nonnegative(value: &f64) -> bool {
    value.is_finite() && *value >= 0.0
}

fn valid_bytes(_value: &ByteCount) -> bool {
    true
}

/// Unsigned byte amount / counter. Serialized as a canonical decimal string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ByteCount(pub u64);

impl Serialize for ByteCount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ByteCount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        bounds::parse_decimal_u64(&text)
            .map(ByteCount)
            .ok_or_else(|| {
                de::Error::custom("byte amounts must be canonical unsigned decimal strings")
            })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
    Valid,
    Unsupported,
    PermissionDenied,
    Unavailable,
    Stale,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reading<T> {
    pub status: ReadingStatus,
    pub value: Option<T>,
    pub sampled_at_ms: Option<i64>,
    pub elapsed_ms: Option<u64>,
    pub scope_id: Option<String>,
    pub reason: Option<String>,
}

impl<T> Reading<T> {
    pub fn valid(
        value: T,
        sampled_at_ms: i64,
        elapsed_ms: u64,
        scope_id: impl Into<String>,
    ) -> Self {
        Self {
            status: ReadingStatus::Valid,
            value: Some(value),
            sampled_at_ms: Some(sampled_at_ms),
            elapsed_ms: Some(elapsed_ms),
            scope_id: Some(scope_id.into()),
            reason: None,
        }
    }

    pub fn unsupported(scope_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::failed(ReadingStatus::Unsupported, scope_id, reason)
    }

    pub fn permission_denied(scope_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::failed(ReadingStatus::PermissionDenied, scope_id, reason)
    }

    pub fn unavailable(scope_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::failed(ReadingStatus::Unavailable, scope_id, reason)
    }

    fn failed(
        status: ReadingStatus,
        scope_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            status,
            value: None,
            sampled_at_ms: None,
            elapsed_ms: None,
            scope_id: Some(scope_id.into()),
            reason: Some(reason.into()),
        }
    }

    pub fn stale(
        value: Option<T>,
        sampled_at_ms: Option<i64>,
        elapsed_ms: Option<u64>,
        scope_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            status: ReadingStatus::Stale,
            value,
            sampled_at_ms,
            elapsed_ms,
            scope_id: Some(scope_id.into()),
            reason: Some(reason.into()),
        }
    }

    pub fn validate_shape(&self, field: &str) -> ModelResult<()> {
        if let Some(reason) = &self.reason {
            validate_text(field, "reason", reason, bounds::MESSAGE_MAX_BYTES)?;
        }
        if let Some(scope_id) = &self.scope_id {
            validate_text(field, "scopeId", scope_id, bounds::LABEL_MAX_BYTES)?;
        }
        match self.status {
            ReadingStatus::Valid => {
                if self.value.is_none() {
                    return fail(format!("{field}: a valid reading requires a value"));
                }
                if self.sampled_at_ms.is_none() || self.elapsed_ms.is_none() {
                    return fail(format!(
                        "{field}: a valid reading requires sampledAtMs and elapsedMs"
                    ));
                }
                if self.reason.is_some() {
                    return fail(format!("{field}: a valid reading must not carry a reason"));
                }
                if self.scope_id.is_none() {
                    return fail(format!("{field}: a valid reading requires a scopeId"));
                }
            }
            ReadingStatus::Stale => {
                if self.reason.is_none() {
                    return fail(format!("{field}: a stale reading requires a reason"));
                }
                if self.value.is_some()
                    && (self.sampled_at_ms.is_none() || self.elapsed_ms.is_none())
                {
                    return fail(format!(
                        "{field}: a stale last-valid value requires its observation timestamps"
                    ));
                }
                if self.value.is_none()
                    && (self.sampled_at_ms.is_some() || self.elapsed_ms.is_some())
                {
                    return fail(format!(
                        "{field}: timestamps are only valid when a stale value exists"
                    ));
                }
            }
            ReadingStatus::Unsupported
            | ReadingStatus::PermissionDenied
            | ReadingStatus::Unavailable => {
                if self.value.is_some() {
                    return fail(format!(
                        "{field}: a {:?} reading must not carry a value",
                        self.status
                    ));
                }
                if self.reason.is_none() {
                    return fail(format!(
                        "{field}: a {:?} reading requires a reason",
                        self.status
                    ));
                }
                if self.sampled_at_ms.is_some() || self.elapsed_ms.is_some() {
                    return fail(format!(
                        "{field}: a {:?} reading must not carry observation timestamps",
                        self.status
                    ));
                }
            }
        }
        if let Some(sampled_at_ms) = self.sampled_at_ms
            && sampled_at_ms < 0
        {
            return fail(format!("{field}: sampledAtMs must not be negative"));
        }
        Ok(())
    }

    pub fn last_valid_value(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

fn validate_reading<T>(
    reading: &Reading<T>,
    field: &str,
    domain: fn(&T) -> bool,
) -> ModelResult<()> {
    reading.validate_shape(field)?;
    if let Some(value) = &reading.value
        && !domain(value)
    {
        return fail(format!("{field}: value is outside its numeric domain"));
    }
    Ok(())
}

fn validate_text(field: &str, part: &str, text: &str, max_bytes: usize) -> ModelResult<()> {
    if !bounds::is_within(text, max_bytes) {
        return fail(format!("{field}: {part} exceeds {max_bytes} UTF-8 bytes"));
    }
    if text.chars().any(char::is_control) {
        return fail(format!("{field}: {part} contains control characters"));
    }
    Ok(())
}

fn validate_opaque_id(field: &str, text: &str) -> ModelResult<()> {
    if text.is_empty() {
        return fail(format!("{field}: identifier must not be empty"));
    }
    validate_text(field, "id", text, bounds::LABEL_MAX_BYTES)
}

fn validate_message(field: &str, text: &str) -> ModelResult<()> {
    if text.is_empty() {
        return fail(format!("{field}: message must not be empty"));
    }
    validate_text(field, "message", text, bounds::MESSAGE_MAX_BYTES)
}

fn validate_scope_ref(
    reading: &Reading<impl Sized>,
    scopes: &[ResourceScope],
    field: &str,
) -> ModelResult<()> {
    if let Some(scope_id) = &reading.scope_id
        && !scopes.iter().any(|scope| &scope.id == scope_id)
    {
        return fail(format!(
            "{field}: scopeId does not reference a discovered scope"
        ));
    }
    Ok(())
}

fn valid_byte_value(reading: &Reading<ByteCount>) -> Option<u64> {
    match reading.status {
        ReadingStatus::Valid => reading.value.map(|value| value.0),
        _ => None,
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Native,
    Wsl2,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostIdentity {
    pub session_id: String,
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub cpu_model: String,
    pub logical_cpu_count: u32,
    pub environment: Environment,
    pub uptime_seconds: Option<u64>,
    pub scope_label: String,
}

impl HostIdentity {
    pub fn validate(&self) -> ModelResult<()> {
        if !is_uuid(&self.session_id) {
            return fail("host.sessionId must be a UUID");
        }
        for (part, value) in [
            ("hostname", &self.hostname),
            ("osName", &self.os_name),
            ("osVersion", &self.os_version),
            ("cpuModel", &self.cpu_model),
            ("scopeLabel", &self.scope_label),
        ] {
            validate_text("host", part, value, bounds::LABEL_MAX_BYTES)?;
        }
        if self.logical_cpu_count == 0 || self.logical_cpu_count > 4096 {
            return fail("host.logicalCpuCount must be between 1 and 4096");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Host,
    CpuPackage,
    Gpu,
    Volume,
    NetworkAggregate,
    Battery,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceScope {
    pub id: String,
    pub kind: ScopeKind,
    pub label: String,
    pub provider: String,
    pub epoch: u64,
}

impl ResourceScope {
    pub fn validate(&self) -> ModelResult<()> {
        validate_opaque_id("scope", &self.id)?;
        validate_text("scope", "label", &self.label, bounds::LABEL_MAX_BYTES)?;
        validate_text(
            "scope",
            "provider",
            &self.provider,
            bounds::PROVIDER_MAX_BYTES,
        )?;
        if !is_json_safe(self.epoch) {
            return fail("scope.epoch must be a JSON-safe integer");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetricUnit {
    Pct,
    Bytes,
    BytesPerSecond,
    Watts,
    Celsius,
    Seconds,
    State,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capability {
    pub metric_id: String,
    pub scope_id: String,
    pub unit: MetricUnit,
    pub provider: String,
    pub status: ReadingStatus,
    pub reason: Option<String>,
}

impl Capability {
    pub fn validate(&self) -> ModelResult<()> {
        if !METRIC_CATALOG.contains(&self.metric_id.as_str()) {
            return fail(format!(
                "capability {} is outside the metric catalog",
                self.metric_id
            ));
        }
        if !bounds::is_within(&self.metric_id, bounds::METRIC_ID_MAX_BYTES) {
            return fail("capability metricId exceeds 48 UTF-8 bytes");
        }
        validate_opaque_id("capability.scopeId", &self.scope_id)?;
        validate_text(
            "capability",
            "provider",
            &self.provider,
            bounds::PROVIDER_MAX_BYTES,
        )?;
        match self.status {
            ReadingStatus::Valid
            | ReadingStatus::Unsupported
            | ReadingStatus::PermissionDenied
            | ReadingStatus::Unavailable => {}
            _ => return fail("capability status must be a current support state"),
        }
        if self.status == ReadingStatus::Valid {
            if self.reason.is_some() {
                return fail("a valid capability must not carry a reason");
            }
        } else if let Some(reason) = &self.reason {
            validate_message("capability", reason)?;
        } else {
            return fail("a non-valid capability requires a reason");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CpuMetrics {
    pub total_pct: Reading<f64>,
    pub user_pct: Reading<f64>,
    pub system_pct: Reading<f64>,
    pub temperature_c: Reading<f64>,
}

impl CpuMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        for (name, reading) in [
            ("totalPct", &self.total_pct),
            ("userPct", &self.user_pct),
            ("systemPct", &self.system_pct),
        ] {
            validate_reading(reading, &format!("cpu.{name}"), valid_pct)?;
            validate_scope_ref(reading, scopes, &format!("cpu.{name}"))?;
        }
        validate_reading(&self.temperature_c, "cpu.temperatureC", valid_temperature)?;
        validate_scope_ref(&self.temperature_c, scopes, "cpu.temperatureC")?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryMetrics {
    pub total_bytes: Reading<ByteCount>,
    pub used_bytes: Reading<ByteCount>,
    pub available_bytes: Reading<ByteCount>,
    pub cached_bytes: Reading<ByteCount>,
    pub used_pct: Reading<f64>,
}

impl MemoryMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        for (name, reading) in [
            ("totalBytes", &self.total_bytes),
            ("usedBytes", &self.used_bytes),
            ("availableBytes", &self.available_bytes),
            ("cachedBytes", &self.cached_bytes),
        ] {
            validate_reading(reading, &format!("memory.{name}"), valid_bytes)?;
            validate_scope_ref(reading, scopes, &format!("memory.{name}"))?;
        }
        validate_reading(&self.used_pct, "memory.usedPct", valid_pct)?;
        validate_scope_ref(&self.used_pct, scopes, "memory.usedPct")?;

        if let (Some(total), Some(used), Some(available)) = (
            valid_byte_value(&self.total_bytes),
            valid_byte_value(&self.used_bytes),
            valid_byte_value(&self.available_bytes),
        ) {
            if used > total || available > total {
                return fail("memory: used/available bytes exceed total capacity");
            }
            if used as u128 + available as u128 > total as u128 {
                return fail("memory: used plus available exceeds total capacity");
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GpuMetrics {
    pub utilization_pct: Reading<f64>,
    pub vram_total_bytes: Reading<ByteCount>,
    pub vram_used_bytes: Reading<ByteCount>,
    pub vram_free_bytes: Reading<ByteCount>,
    pub vram_reserved_bytes: Reading<ByteCount>,
    pub temperature_c: Reading<f64>,
    pub power_w: Reading<f64>,
    pub fan_pct: Reading<f64>,
    pub power_limit_pct: Reading<f64>,
}

impl GpuMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        for (name, reading) in [
            ("utilizationPct", &self.utilization_pct),
            ("fanPct", &self.fan_pct),
            ("powerLimitPct", &self.power_limit_pct),
        ] {
            validate_reading(reading, &format!("gpu.{name}"), valid_pct)?;
            validate_scope_ref(reading, scopes, &format!("gpu.{name}"))?;
        }
        for (name, reading) in [
            ("vramTotalBytes", &self.vram_total_bytes),
            ("vramUsedBytes", &self.vram_used_bytes),
            ("vramFreeBytes", &self.vram_free_bytes),
            ("vramReservedBytes", &self.vram_reserved_bytes),
        ] {
            validate_reading(reading, &format!("gpu.{name}"), valid_bytes)?;
            validate_scope_ref(reading, scopes, &format!("gpu.{name}"))?;
        }
        for (name, reading) in [
            ("temperatureC", &self.temperature_c),
            ("powerW", &self.power_w),
        ] {
            let domain: fn(&f64) -> bool = if name == "powerW" {
                valid_nonnegative
            } else {
                valid_temperature
            };
            validate_reading(reading, &format!("gpu.{name}"), domain)?;
            validate_scope_ref(reading, scopes, &format!("gpu.{name}"))?;
        }

        let total = valid_byte_value(&self.vram_total_bytes);
        let used = valid_byte_value(&self.vram_used_bytes);
        let free = valid_byte_value(&self.vram_free_bytes);
        let reserved = valid_byte_value(&self.vram_reserved_bytes);
        if let Some(total) = total {
            for (label, value) in [("used", used), ("free", free), ("reserved", reserved)] {
                if let Some(value) = value
                    && value > total
                {
                    return fail(format!("gpu: vram {label} exceeds total capacity"));
                }
            }
            let sum: u128 = [used, free, reserved]
                .iter()
                .flatten()
                .map(|v| *v as u128)
                .sum();
            if sum > total as u128 {
                return fail("gpu: VRAM categories exceed total capacity");
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiskMetrics {
    pub total_bytes: Reading<ByteCount>,
    pub free_bytes: Reading<ByteCount>,
    pub read_bytes_per_second: Reading<f64>,
    pub write_bytes_per_second: Reading<f64>,
    pub written_session_bytes: Reading<ByteCount>,
}

impl DiskMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        for (name, reading) in [
            ("totalBytes", &self.total_bytes),
            ("freeBytes", &self.free_bytes),
            ("writtenSessionBytes", &self.written_session_bytes),
        ] {
            validate_reading(reading, &format!("disk.{name}"), valid_bytes)?;
            validate_scope_ref(reading, scopes, &format!("disk.{name}"))?;
        }
        for (name, reading) in [
            ("readBytesPerSecond", &self.read_bytes_per_second),
            ("writeBytesPerSecond", &self.write_bytes_per_second),
        ] {
            validate_reading(reading, &format!("disk.{name}"), valid_nonnegative)?;
            validate_scope_ref(reading, scopes, &format!("disk.{name}"))?;
        }
        if let (Some(total), Some(free)) = (
            valid_byte_value(&self.total_bytes),
            valid_byte_value(&self.free_bytes),
        ) && free > total
        {
            return fail("disk: free bytes exceed total capacity");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkMetrics {
    pub receive_bytes_per_second: Reading<f64>,
    pub send_bytes_per_second: Reading<f64>,
    pub received_session_bytes: Reading<ByteCount>,
    pub sent_session_bytes: Reading<ByteCount>,
}

impl NetworkMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        for (name, reading) in [
            ("receiveBytesPerSecond", &self.receive_bytes_per_second),
            ("sendBytesPerSecond", &self.send_bytes_per_second),
        ] {
            validate_reading(reading, &format!("network.{name}"), valid_nonnegative)?;
            validate_scope_ref(reading, scopes, &format!("network.{name}"))?;
        }
        for (name, reading) in [
            ("receivedSessionBytes", &self.received_session_bytes),
            ("sentSessionBytes", &self.sent_session_bytes),
        ] {
            validate_reading(reading, &format!("network.{name}"), valid_bytes)?;
            validate_scope_ref(reading, scopes, &format!("network.{name}"))?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowerPrimaryKind {
    BatteryPct,
    GpuWatts,
    CpuPackageWatts,
    Unavailable,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatteryState {
    Charging,
    Discharging,
    Full,
    OnAc,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PowerMetrics {
    pub primary_kind: PowerPrimaryKind,
    pub primary_value: Reading<f64>,
    pub watts: Reading<f64>,
    pub cpu_package_w: Reading<f64>,
    pub gpu_board_w: Reading<f64>,
    pub gpu_temperature_c: Reading<f64>,
    pub fan_pct: Reading<f64>,
    pub power_limit_pct: Reading<f64>,
    pub battery_charge_pct: Reading<f64>,
    pub battery_health_pct: Reading<f64>,
    pub battery_remaining_seconds: Reading<u64>,
    pub battery_state: Reading<BatteryState>,
}

impl PowerMetrics {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        let primary_domain: fn(&f64) -> bool = match self.primary_kind {
            PowerPrimaryKind::BatteryPct => valid_pct,
            _ => valid_nonnegative,
        };
        validate_reading(&self.primary_value, "power.primaryValue", primary_domain)?;
        validate_scope_ref(&self.primary_value, scopes, "power.primaryValue")?;

        for (name, reading) in [
            ("watts", &self.watts),
            ("cpuPackageW", &self.cpu_package_w),
            ("gpuBoardW", &self.gpu_board_w),
        ] {
            validate_reading(reading, &format!("power.{name}"), valid_nonnegative)?;
            validate_scope_ref(reading, scopes, &format!("power.{name}"))?;
        }
        validate_reading(
            &self.gpu_temperature_c,
            "power.gpuTemperatureC",
            valid_temperature,
        )?;
        validate_scope_ref(&self.gpu_temperature_c, scopes, "power.gpuTemperatureC")?;
        for (name, reading) in [
            ("fanPct", &self.fan_pct),
            ("powerLimitPct", &self.power_limit_pct),
            ("batteryChargePct", &self.battery_charge_pct),
            ("batteryHealthPct", &self.battery_health_pct),
        ] {
            validate_reading(reading, &format!("power.{name}"), valid_pct)?;
            validate_scope_ref(reading, scopes, &format!("power.{name}"))?;
        }
        validate_reading(
            &self.battery_remaining_seconds,
            "power.batteryRemainingSeconds",
            |_| true,
        )?;
        validate_scope_ref(
            &self.battery_remaining_seconds,
            scopes,
            "power.batteryRemainingSeconds",
        )?;
        validate_reading(&self.battery_state, "power.batteryState", |_| true)?;
        validate_scope_ref(&self.battery_state, scopes, "power.batteryState")?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrentSample {
    pub sequence: u64,
    pub captured_at_ms: i64,
    pub elapsed_ms: u64,
    pub interval_ms: u64,
    pub host: HostIdentity,
    pub scopes: Vec<ResourceScope>,
    pub capabilities: Vec<Capability>,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub gpu: GpuMetrics,
    pub disk: DiskMetrics,
    pub network: NetworkMetrics,
    pub power: PowerMetrics,
}

impl CurrentSample {
    pub fn validate(&self) -> ModelResult<()> {
        if !is_json_safe(self.sequence) {
            return fail("sample.sequence must be a JSON-safe integer");
        }
        if self.captured_at_ms < 0 {
            return fail("sample.capturedAtMs must not be negative");
        }
        if !matches!(self.interval_ms, 1000 | 2000 | 5000) {
            return fail("sample.intervalMs must be 1000, 2000 or 5000");
        }
        self.host.validate()?;

        if self.scopes.is_empty() || self.scopes.len() > MAX_SCOPES {
            return fail(format!(
                "sample.scopes must contain 1..={MAX_SCOPES} scopes"
            ));
        }
        for (index, scope) in self.scopes.iter().enumerate() {
            scope.validate()?;
            if self.scopes[..index]
                .iter()
                .any(|other| other.id == scope.id)
            {
                return fail(format!("sample.scopes contains duplicate id {}", scope.id));
            }
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return fail(format!(
                "sample.capabilities must not exceed {MAX_CAPABILITIES} entries"
            ));
        }
        for capability in &self.capabilities {
            capability.validate()?;
            if !self
                .scopes
                .iter()
                .any(|scope| scope.id == capability.scope_id)
            {
                return fail(format!(
                    "capability {} references an undiscovered scope",
                    capability.metric_id
                ));
            }
        }

        self.cpu.validate(&self.scopes)?;
        self.memory.validate(&self.scopes)?;
        self.gpu.validate(&self.scopes)?;
        self.disk.validate(&self.scopes)?;
        self.network.validate(&self.scopes)?;
        self.power.validate(&self.scopes)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentRole {
    Used,
    Available,
    Free,
    Reserved,
    Unclassified,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessObservation {
    pub key: String,
    pub pid: u32,
    pub name: String,
    pub resident_bytes: Reading<ByteCount>,
    pub cpu_pct: Reading<f64>,
    pub gpu_pct: Reading<f64>,
    pub power_w: Reading<f64>,
}

impl ProcessObservation {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        validate_opaque_id("process.key", &self.key)?;
        validate_text("process", "name", &self.name, bounds::LABEL_MAX_BYTES)?;
        validate_reading(&self.resident_bytes, "process.residentBytes", valid_bytes)?;
        validate_scope_ref(&self.resident_bytes, scopes, "process.residentBytes")?;
        for (name, reading) in [("cpuPct", &self.cpu_pct), ("gpuPct", &self.gpu_pct)] {
            validate_reading(reading, &format!("process.{name}"), valid_pct)?;
            validate_scope_ref(reading, scopes, &format!("process.{name}"))?;
        }
        validate_reading(&self.power_w, "process.powerW", valid_nonnegative)?;
        validate_scope_ref(&self.power_w, scopes, "process.powerW")?;
        Ok(())
    }

    fn valid_metric(&self, metric: RankingMetric) -> Option<f64> {
        match metric {
            RankingMetric::Cpu => valid_reading_value(&self.cpu_pct),
            RankingMetric::Gpu => valid_reading_value(&self.gpu_pct),
            RankingMetric::Power => valid_reading_value(&self.power_w),
        }
    }
}

fn valid_reading_value(reading: &Reading<f64>) -> Option<f64> {
    match reading.status {
        ReadingStatus::Valid => reading.value,
        _ => None,
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RankingMetric {
    Cpu,
    Gpu,
    Power,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSummaries {
    pub memory_top: Vec<ProcessObservation>,
    pub cpu_top: Vec<ProcessObservation>,
    pub gpu_top: Vec<ProcessObservation>,
    pub power_top: Vec<ProcessObservation>,
    pub supported_ranking_metrics: Vec<RankingMetric>,
    pub coverage_notes: Vec<String>,
    pub displayed_memory_total_bytes: ByteCount,
}

impl ProcessSummaries {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        if self.memory_top.len() > MAX_PROCESS_MEMORY_ENTRIES {
            return fail(format!(
                "processSummaries.memoryTop must not exceed {MAX_PROCESS_MEMORY_ENTRIES} entries"
            ));
        }
        for (name, list) in [
            ("cpuTop", &self.cpu_top),
            ("gpuTop", &self.gpu_top),
            ("powerTop", &self.power_top),
        ] {
            if list.len() > MAX_PROCESS_RANK_ENTRIES {
                return fail(format!(
                    "processSummaries.{name} must not exceed {MAX_PROCESS_RANK_ENTRIES} entries"
                ));
            }
        }
        for observation in self
            .memory_top
            .iter()
            .chain(&self.cpu_top)
            .chain(&self.gpu_top)
            .chain(&self.power_top)
        {
            observation.validate(scopes)?;
        }
        validate_process_order(&self.memory_top, "memory", |observation| {
            valid_byte_value(&observation.resident_bytes)
        })?;
        validate_process_order(&self.cpu_top, "cpu", |observation| {
            observation.valid_metric(RankingMetric::Cpu)
        })?;
        validate_process_order(&self.gpu_top, "gpu", |observation| {
            observation.valid_metric(RankingMetric::Gpu)
        })?;
        validate_process_order(&self.power_top, "power", |observation| {
            observation.valid_metric(RankingMetric::Power)
        })?;

        let mut seen = Vec::new();
        for metric in &self.supported_ranking_metrics {
            if seen.contains(metric) {
                return fail("processSummaries.supportedRankingMetrics contains duplicates");
            }
            seen.push(*metric);
        }
        if self.coverage_notes.len() > bounds::MAX_NOTICES {
            return fail("processSummaries.coverageNotes must not exceed 8 entries");
        }
        for note in &self.coverage_notes {
            validate_message("processSummaries.coverageNotes", note)?;
        }
        let Some(displayed) = self
            .memory_top
            .iter()
            .filter_map(|observation| valid_byte_value(&observation.resident_bytes))
            .try_fold(0u64, u64::checked_add)
        else {
            return fail("processSummaries.displayedMemoryTotalBytes exceeds the u64 domain");
        };
        if displayed != self.displayed_memory_total_bytes.0 {
            return fail(
                "processSummaries.displayedMemoryTotalBytes must equal the displayed resident sum",
            );
        }
        Ok(())
    }
}

fn validate_process_order<T: PartialOrd + Copy>(
    list: &[ProcessObservation],
    label: &str,
    value: impl Fn(&ProcessObservation) -> Option<T>,
) -> ModelResult<()> {
    let mut previous: Option<(T, &str)> = None;
    for observation in list {
        let Some(current) = value(observation) else {
            return fail(format!(
                "processSummaries: an entry in the {label} list lacks its ranking metric"
            ));
        };
        if let Some((previous_value, previous_key)) = previous
            && (current > previous_value
                || (current == previous_value && observation.key.as_str() <= previous_key))
        {
            return fail(format!(
                "processSummaries: the {label} list is not ordered by raw value then key"
            ));
        }
        previous = Some((current, observation.key.as_str()));
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BreakdownCoverage {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySegment {
    pub id: String,
    pub label: String,
    pub bytes: ByteCount,
    pub role: SegmentRole,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryBreakdown {
    pub kind: MemoryView,
    pub scope_id: String,
    pub total_bytes: ByteCount,
    pub used_pct: f64,
    pub segments: Vec<MemorySegment>,
    pub coverage: BreakdownCoverage,
    pub note: Option<String>,
}

impl MemoryBreakdown {
    fn validate(&self, expected_kind: MemoryView) -> ModelResult<()> {
        if self.kind != expected_kind {
            return fail("memoryBreakdowns: kind does not match its RAM/VRAM slot");
        }
        validate_opaque_id("memoryBreakdown.scopeId", &self.scope_id)?;
        if !valid_pct(&self.used_pct) {
            return fail("memoryBreakdown.usedPct is outside 0..=100");
        }
        if self.total_bytes.0 == 0 {
            return fail("memoryBreakdown.totalBytes must be positive");
        }
        if self.segments.len() > MAX_BREAKDOWN_SEGMENTS {
            return fail("memoryBreakdown.segments must not exceed 8 entries");
        }
        let mut sum: u128 = 0;
        let mut ids = Vec::new();
        for segment in &self.segments {
            validate_opaque_id("memoryBreakdown.segment.id", &segment.id)?;
            validate_text(
                "memoryBreakdown.segment",
                "label",
                &segment.label,
                bounds::LABEL_MAX_BYTES,
            )?;
            if ids.contains(&segment.id) {
                return fail("memoryBreakdown.segments contains duplicate ids");
            }
            ids.push(segment.id.clone());
            sum += segment.bytes.0 as u128;
        }
        if sum > self.total_bytes.0 as u128 {
            return fail("memoryBreakdown segments exceed total capacity");
        }
        if self.coverage == BreakdownCoverage::Complete && sum != self.total_bytes.0 as u128 {
            return fail("a complete memoryBreakdown must partition total capacity");
        }
        if self.coverage == BreakdownCoverage::Unavailable && !self.segments.is_empty() {
            return fail("an unavailable memoryBreakdown must not carry segments");
        }
        if let Some(note) = &self.note {
            validate_message("memoryBreakdown.note", note)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryBreakdowns {
    pub ram: Option<MemoryBreakdown>,
    pub vram: Option<MemoryBreakdown>,
}

impl MemoryBreakdowns {
    fn validate(&self) -> ModelResult<()> {
        if let Some(ram) = &self.ram {
            ram.validate(MemoryView::Ram)?;
        }
        if let Some(vram) = &self.vram {
            vram.validate(MemoryView::Vram)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    #[serde(rename = "1m")]
    OneMinute,
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "15m")]
    FifteenMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl Range {
    pub fn duration_ms(self) -> u64 {
        match self {
            Self::OneMinute => 60_000,
            Self::FiveMinutes => 300_000,
            Self::FifteenMinutes => 900_000,
            Self::OneHour => 3_600_000,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryView {
    Ram,
    Vram,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Live,
    Paused,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SeriesId {
    CpuPct,
    RamUsedPct,
    GpuPct,
    DiskBytesPerSecond,
    NetworkBytesPerSecond,
    PowerW,
}

impl SeriesId {
    pub fn expect_unit(self) -> MetricUnit {
        match self {
            Self::CpuPct | Self::RamUsedPct | Self::GpuPct => MetricUnit::Pct,
            Self::DiskBytesPerSecond | Self::NetworkBytesPerSecond => MetricUnit::BytesPerSecond,
            Self::PowerW => MetricUnit::Watts,
        }
    }

    pub fn wire_name(self) -> &'static str {
        match self {
            Self::CpuPct => "cpuPct",
            Self::RamUsedPct => "ramUsedPct",
            Self::GpuPct => "gpuPct",
            Self::DiskBytesPerSecond => "diskBytesPerSecond",
            Self::NetworkBytesPerSecond => "networkBytesPerSecond",
            Self::PowerW => "powerW",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartColumn {
    pub value: Option<f64>,
    pub sample_count: u32,
    pub has_gap: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Chart {
    pub series_id: SeriesId,
    pub unit: MetricUnit,
    pub scope_id: Option<String>,
    pub scope_label: Option<String>,
    pub scale_max: f64,
    pub columns: Vec<ChartColumn>,
    pub partial_history: bool,
    pub status: ReadingStatus,
}

impl Chart {
    fn validate(&self, scopes: &[ResourceScope]) -> ModelResult<()> {
        let field = format!("history.charts.{}", self.series_id.wire_name());
        if self.unit != self.series_id.expect_unit() {
            return fail(format!("{field}: unit does not match the series"));
        }
        if let Some(scope_id) = &self.scope_id {
            validate_opaque_id(&format!("{field}.scopeId"), scope_id)?;
            if !scopes.is_empty() && !scopes.iter().any(|scope| &scope.id == scope_id) {
                return fail(format!(
                    "{field}: scopeId does not reference a discovered scope"
                ));
            }
        }
        if let Some(label) = &self.scope_label {
            validate_text(&field, "scopeLabel", label, bounds::LABEL_MAX_BYTES)?;
        }
        if !self.scale_max.is_finite() || self.scale_max < 1.0 {
            return fail(format!(
                "{field}: scaleMax must be a finite value of at least 1"
            ));
        }
        let percent_series = self.unit == MetricUnit::Pct;
        if percent_series && self.scale_max != 100.0 {
            return fail(format!("{field}: percent charts must use a 0-100 scale"));
        }
        if self.columns.len() != CHART_COLUMNS {
            return fail(format!(
                "{field}: expected exactly {CHART_COLUMNS} columns, found {}",
                self.columns.len()
            ));
        }
        let mut largest = 0.0_f64;
        let mut sampled = 0usize;
        for column in &self.columns {
            match column.value {
                Some(value) => {
                    if column.sample_count == 0 {
                        return fail(format!("{field}: a valued column requires samples"));
                    }
                    let valid = if percent_series {
                        valid_pct(&value)
                    } else {
                        valid_nonnegative(&value)
                    };
                    if !valid {
                        return fail(format!("{field}: a column value is outside its domain"));
                    }
                    largest = largest.max(value);
                    sampled += 1;
                }
                None => {
                    if column.sample_count != 0 {
                        return fail(format!(
                            "{field}: an empty column must have zero sampleCount"
                        ));
                    }
                    if !column.has_gap {
                        return fail(format!("{field}: an empty column must be marked as a gap"));
                    }
                }
            }
        }
        if !percent_series && self.scale_max < largest {
            return fail(format!(
                "{field}: scaleMax is below the largest column value"
            ));
        }
        match self.status {
            ReadingStatus::Valid => {
                if sampled == 0 {
                    return fail(format!(
                        "{field}: a valid chart requires at least one sample"
                    ));
                }
            }
            ReadingStatus::Unavailable => {
                if sampled != 0 {
                    return fail(format!(
                        "{field}: an unavailable chart must not carry samples"
                    ));
                }
            }
            _ => {
                return fail(format!(
                    "{field}: chart status must be valid or unavailable"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryProjection {
    pub range: Range,
    pub endpoint_elapsed_ms: u64,
    pub retained_from_elapsed_ms: Option<u64>,
    pub charts: Vec<Chart>,
    pub gpu_sample_average_pct: Reading<f64>,
    pub gpu_sample_peak_pct: Reading<f64>,
}

impl HistoryProjection {
    fn validate(
        &self,
        scopes: &[ResourceScope],
        selection: &ViewSelection,
        endpoint: u64,
    ) -> ModelResult<()> {
        if self.range != selection.range {
            return fail("history.range must match the selected range");
        }
        if self.endpoint_elapsed_ms != endpoint {
            return fail("history.endpointElapsedMs must equal the view endpoint");
        }
        if let Some(from) = self.retained_from_elapsed_ms
            && from > self.endpoint_elapsed_ms
        {
            return fail("history.retainedFromElapsedMs must not exceed the endpoint");
        }
        if self.charts.len() != CHART_SERIES_IDS.len() {
            return fail("history must contain exactly six charts");
        }
        for series in CHART_SERIES_IDS {
            if !self
                .charts
                .iter()
                .any(|chart| chart.series_id.wire_name() == series)
            {
                return fail(format!("history is missing the {series} series"));
            }
        }
        for (index, chart) in self.charts.iter().enumerate() {
            chart.validate(scopes)?;
            if self.charts[..index]
                .iter()
                .any(|other| other.series_id == chart.series_id)
            {
                return fail("history contains duplicate series ids");
            }
        }
        validate_reading(
            &self.gpu_sample_average_pct,
            "history.gpuSampleAveragePct",
            valid_pct,
        )?;
        validate_reading(
            &self.gpu_sample_peak_pct,
            "history.gpuSamplePeakPct",
            valid_pct,
        )?;
        validate_scope_ref(
            &self.gpu_sample_average_pct,
            scopes,
            "history.gpuSampleAveragePct",
        )?;
        validate_scope_ref(
            &self.gpu_sample_peak_pct,
            scopes,
            "history.gpuSamplePeakPct",
        )?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewSelection {
    pub range: Range,
    pub memory_view: MemoryView,
    pub ranking_metric: RankingMetric,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionPreferences {
    pub interval_ms: u64,
    pub selected_gpu_id: Option<String>,
}

impl CollectionPreferences {
    fn validate(&self) -> ModelResult<()> {
        if !matches!(self.interval_ms, 1000 | 2000 | 5000) {
            return fail("preferences.intervalMs must be 1000, 2000 or 5000");
        }
        if let Some(gpu_id) = &self.selected_gpu_id {
            validate_opaque_id("preferences.selectedGpuId", gpu_id)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notice {
    pub code: String,
    pub message: String,
}

impl Notice {
    fn validate(&self) -> ModelResult<()> {
        validate_text("notice", "code", &self.code, bounds::PROVIDER_MAX_BYTES)?;
        validate_message("notice.message", &self.message)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct View {
    pub contract_version: u32,
    pub session_id: String,
    pub revision: u64,
    pub mode: Mode,
    pub visible: bool,
    pub inspection_id: Option<String>,
    pub endpoint_elapsed_ms: u64,
    pub endpoint_at_ms: i64,
    pub sample: Option<CurrentSample>,
    pub process_summaries: Option<ProcessSummaries>,
    pub memory_breakdowns: MemoryBreakdowns,
    pub history: Option<HistoryProjection>,
    pub selection: ViewSelection,
    pub preferences: CollectionPreferences,
    pub notices: Vec<Notice>,
}

impl View {
    pub fn validate(&self) -> ModelResult<()> {
        if self.contract_version != CONTRACT_VERSION {
            return fail("view.contractVersion must be 1");
        }
        if !is_uuid(&self.session_id) {
            return fail("view.sessionId must be a UUID");
        }
        if !is_json_safe(self.revision) {
            return fail("view.revision must be a JSON-safe integer");
        }
        if self.endpoint_at_ms < 0 {
            return fail("view.endpointAtMs must not be negative");
        }
        match self.mode {
            Mode::Live => {
                if self.inspection_id.is_some() {
                    return fail("view.inspectionId must be null while live");
                }
            }
            Mode::Paused => match &self.inspection_id {
                Some(id) if is_uuid(id) => {}
                _ => return fail("view.inspectionId must be a UUID while paused"),
            },
        }
        self.preferences.validate()?;
        if self.notices.len() > bounds::MAX_NOTICES {
            return fail("view.notices must not exceed 8 entries");
        }
        for notice in &self.notices {
            notice.validate()?;
        }

        let scopes = self
            .sample
            .as_ref()
            .map(|sample| sample.scopes.as_slice())
            .unwrap_or(&[]);
        if let Some(sample) = &self.sample {
            sample.validate()?;
            if sample.host.session_id != self.session_id {
                return fail("sample.host.sessionId must match the view session");
            }
            if sample.elapsed_ms > self.endpoint_elapsed_ms {
                return fail("view.endpointElapsedMs must not precede the sample");
            }
        }
        if let Some(summaries) = &self.process_summaries {
            summaries.validate(scopes)?;
        }
        self.memory_breakdowns.validate()?;
        if let Some(history) = &self.history {
            history.validate(scopes, &self.selection, self.endpoint_elapsed_ms)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuKind {
    Discrete,
    Integrated,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GpuDescriptor {
    pub id: String,
    pub name: String,
    pub vendor: GpuVendor,
    pub kind: GpuKind,
    pub available: bool,
}

impl GpuDescriptor {
    fn validate(&self) -> ModelResult<()> {
        validate_opaque_id("gpu.id", &self.id)?;
        validate_text("gpu", "name", &self.name, bounds::LABEL_MAX_BYTES)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bootstrap {
    pub view: View,
    pub gpus: Vec<GpuDescriptor>,
}

impl Bootstrap {
    pub fn validate(&self) -> ModelResult<()> {
        self.view.validate()?;
        if self.gpus.len() > MAX_GPUS {
            return fail(format!("bootstrap.gpus must not exceed {MAX_GPUS} entries"));
        }
        for (index, gpu) in self.gpus.iter().enumerate() {
            gpu.validate()?;
            if self.gpus[..index].iter().any(|other| other.id == gpu.id) {
                return fail("bootstrap.gpus contains duplicate ids");
            }
        }
        match &self.view.preferences.selected_gpu_id {
            Some(selected) => {
                if !self.gpus.iter().any(|gpu| &gpu.id == selected) {
                    return fail("preferences.selectedGpuId must match a discovered GPU");
                }
            }
            None => {
                if !self.gpus.is_empty() {
                    return fail("preferences.selectedGpuId must name a discovered GPU");
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub contract_version: u32,
    pub session_id: String,
    pub subscription_id: String,
    pub delivery_sequence: u64,
    pub view: View,
}

impl Delivery {
    pub fn validate(&self) -> ModelResult<()> {
        if self.contract_version != CONTRACT_VERSION {
            return fail("delivery.contractVersion must be 1");
        }
        if !is_uuid(&self.session_id) {
            return fail("delivery.sessionId must be a UUID");
        }
        if !is_uuid(&self.subscription_id) {
            return fail("delivery.subscriptionId must be a UUID");
        }
        if self.delivery_sequence == 0 || !is_json_safe(self.delivery_sequence) {
            return fail("delivery.deliverySequence must be a positive JSON-safe integer");
        }
        if self.view.session_id != self.session_id {
            return fail("delivery.sessionId must match its view");
        }
        self.view.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_and_json_safe_bounds_are_exact() {
        assert!(is_uuid("0f7a1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b"));
        assert!(!is_uuid("not-a-uuid"));
        assert!(!is_uuid("0f7a1c2e3b4d4e5f8a9b0c1d2e3f4a5b"));
        assert!(is_json_safe(MAX_JSON_SAFE_INT));
        assert!(!is_json_safe(MAX_JSON_SAFE_INT + 1));
    }

    #[test]
    fn byte_counts_serialize_as_decimal_strings() {
        let value = serde_json::to_value(ByteCount(u64::MAX)).unwrap();
        assert_eq!(
            value,
            serde_json::Value::String("18446744073709551615".into())
        );
        assert!(serde_json::from_value::<ByteCount>(serde_json::json!("01")).is_err());
        assert!(serde_json::from_value::<ByteCount>(serde_json::json!(12)).is_err());
    }

    #[test]
    fn numeric_domains_reject_nan_and_out_of_range_values() {
        assert!(valid_pct(&0.0));
        assert!(valid_pct(&100.0));
        assert!(!valid_pct(&100.1));
        assert!(!valid_pct(&f64::NAN));
        assert!(!valid_nonnegative(&-0.1));
        assert!(!valid_nonnegative(&f64::INFINITY));
        assert!(valid_temperature(&-273.15));
    }
}
