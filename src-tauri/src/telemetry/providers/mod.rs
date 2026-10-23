//! Long-lived provider boundary: immutable, normalized polls with per-field error mapping.

#[cfg(feature = "test-fixtures")]
pub mod fixtures;

pub mod linux;
pub mod network;
pub mod nvml;
pub mod power;
pub mod system;
pub mod windows;

use crate::telemetry::clock::Clock;
use crate::telemetry::model::{
    CpuMetrics, DiskMetrics, GpuMetrics, MemoryMetrics, NetworkMetrics, PowerMetrics, Reading,
    ReadingStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderError {
    Unsupported,
    PermissionDenied,
    Invalid,
    Unavailable,
}

impl ProviderError {
    pub fn status(self) -> ReadingStatus {
        match self {
            Self::Unsupported => ReadingStatus::Unsupported,
            Self::PermissionDenied => ReadingStatus::PermissionDenied,
            Self::Invalid | Self::Unavailable => ReadingStatus::Unavailable,
        }
    }

    /// Curated, bounded text only; raw provider errors never reach the wire.
    pub fn reading<T>(self, scope_id: &str, reason: &str) -> Reading<T> {
        Reading {
            status: self.status(),
            value: None,
            sampled_at_ms: None,
            elapsed_ms: None,
            scope_id: Some(scope_id.to_owned()),
            reason: Some(crate::telemetry::bounds::bounded_message(reason)),
        }
    }
}

pub struct PollContext<'a> {
    pub clock: &'a dyn Clock,
    pub elapsed_ms: u64,
    pub interval_ms: u64,
}

impl PollContext<'_> {
    pub fn sampled_at_ms(&self) -> i64 {
        self.clock.wall_ms()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderLoss {
    pub provider: String,
    pub reason: String,
}

/// One immutable, normalized poll result. Groups stay independent so one failing sensor does not
/// erase unrelated valid readings.
/// One measured selected-adapter attribution for an existing process list entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessGpuAttribution {
    pub pid: u32,
    pub reading: Reading<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderPoll {
    pub cpu: Option<CpuMetrics>,
    pub memory: Option<MemoryMetrics>,
    pub gpu: Option<GpuMetrics>,
    pub disk: Option<DiskMetrics>,
    pub network: Option<NetworkMetrics>,
    pub power: Option<PowerMetrics>,
    pub losses: Vec<ProviderLoss>,
    pub processes: Vec<crate::telemetry::model::ProcessObservation>,
    pub process_gpu: Vec<ProcessGpuAttribution>,
    pub network_members: Option<crate::telemetry::bounds::NetworkMemberMetadata>,
}

pub trait Provider: Send + Sync {
    fn provider_id(&self) -> &'static str;

    /// Must not block the UI thread; the collector owns the serialized polling thread.
    fn poll(&self, context: &PollContext<'_>) -> ProviderPoll;
}

impl From<std::io::Error> for ProviderError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Self::PermissionDenied
        } else {
            Self::Unavailable
        }
    }
}

pub fn observed<T>(
    value: Result<T, ProviderError>,
    context: &PollContext<'_>,
    scope: &str,
) -> Reading<T> {
    match value {
        Ok(v) => Reading::valid(v, context.sampled_at_ms(), context.elapsed_ms, scope),
        Err(e) => e.reading(
            scope,
            match e {
                ProviderError::PermissionDenied => "Access to this reading was denied",
                ProviderError::Unsupported => "This sensor is not supported",
                _ => "No valid observation is available",
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_errors_map_to_explicit_reading_states() {
        let denied: Reading<f64> =
            ProviderError::PermissionDenied.reading("gpu:test", "Driver denied the query");
        assert_eq!(denied.status, ReadingStatus::PermissionDenied);
        assert!(denied.value.is_none());
        assert!(denied.sampled_at_ms.is_none());
        assert_eq!(denied.reason.as_deref(), Some("Driver denied the query"));
        assert_eq!(
            ProviderError::Invalid.status(),
            ReadingStatus::Unavailable,
            "invalid observations are unavailable, never fabricated zeros"
        );
    }
}
