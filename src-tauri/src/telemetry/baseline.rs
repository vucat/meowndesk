//! FR-023 support acceptance is independent of optional sensors and never mutates readings.
use crate::telemetry::{bounds, model::*};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineState {
    Initializing,
    Ready,
    Degraded,
}
#[derive(Clone, Debug)]
pub struct BaselineResult {
    pub state: BaselineState,
    pub notice: Option<Notice>,
    pub native_accepted: bool,
    pub failed: Vec<&'static str>,
}
fn valid<T>(reading: &Reading<T>) -> bool {
    reading.status == ReadingStatus::Valid && reading.value.is_some()
}
pub fn evaluate(
    sample: &CurrentSample,
    processes: &[ProcessObservation],
    warmed_up: bool,
) -> BaselineResult {
    if !warmed_up {
        return BaselineResult {
            state: BaselineState::Initializing,
            notice: None,
            native_accepted: false,
            failed: vec![],
        };
    }
    let mut failed = Vec::new();
    for (metric, ok) in [
        (
            "cpu.totalPct",
            valid(&sample.cpu.total_pct)
                && sample.cpu.total_pct.value.as_ref().is_some_and(valid_pct),
        ),
        ("memory.usedBytes", valid(&sample.memory.used_bytes)),
        (
            "memory.totalBytes",
            valid(&sample.memory.total_bytes)
                && sample.memory.total_bytes.value.is_some_and(|v| v.0 > 0),
        ),
        ("disk.freeBytes", valid(&sample.disk.free_bytes)),
        (
            "disk.totalBytes",
            valid(&sample.disk.total_bytes)
                && sample.disk.total_bytes.value.is_some_and(|v| v.0 > 0),
        ),
        (
            "network.receiveBytesPerSecond",
            valid(&sample.network.receive_bytes_per_second),
        ),
        (
            "network.sendBytesPerSecond",
            valid(&sample.network.send_bytes_per_second),
        ),
    ] {
        if !ok {
            failed.push(metric);
        }
    }
    // Both readings must belong to the same readable process, rather than two partial PIDs.
    if !processes
        .iter()
        .any(|p| valid(&p.cpu_pct) && valid(&p.resident_bytes))
    {
        if !processes.iter().any(|p| valid(&p.cpu_pct)) {
            failed.push("process.cpuPct");
        }
        if !processes.iter().any(|p| valid(&p.resident_bytes)) {
            failed.push("process.residentBytes");
        }
        if !failed.iter().any(|m| m.starts_with("process.")) {
            failed.push("process.cpuPct+residentBytes");
        }
    }
    let ready = failed.is_empty();
    let notice = (!ready).then(|| Notice {
        code: "baseline_degraded".into(),
        message: bounds::bounded_message(&format!("Degraded monitoring: {}", failed.join(", "))),
    });
    BaselineResult {
        state: if ready {
            BaselineState::Ready
        } else {
            BaselineState::Degraded
        },
        notice,
        native_accepted: ready && sample.host.environment == Environment::Native,
        failed,
    }
}
