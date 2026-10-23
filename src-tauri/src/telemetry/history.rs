//! One scalar-only session ring. No samples, process tables or projections are retained here.
use crate::telemetry::{model::*, normalize::combined_rate};
use std::collections::VecDeque;
pub const MAX_FRAMES: usize = 3601;
pub const RETENTION_MS: u64 = 3_600_000;
pub const SERIES: [SeriesId; 6] = [
    SeriesId::CpuPct,
    SeriesId::RamUsedPct,
    SeriesId::GpuPct,
    SeriesId::DiskBytesPerSecond,
    SeriesId::NetworkBytesPerSecond,
    SeriesId::PowerW,
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesScope {
    pub id: Option<String>,
    pub epoch: u64,
}
#[derive(Clone, Debug)]
pub struct Scalar {
    pub value: Option<f64>,
    pub scope: SeriesScope,
}
#[derive(Clone, Debug)]
pub struct ScalarFrame {
    pub sequence: u64,
    pub captured_at_ms: i64,
    pub elapsed_ms: u64,
    pub interval_ms: u64,
    pub discontinuity: bool,
    pub series: [Scalar; 6],
}
pub fn series_readings(s: &CurrentSample) -> [Reading<f64>; 6] {
    [
        s.cpu.total_pct.clone(),
        s.memory.used_pct.clone(),
        s.gpu.utilization_pct.clone(),
        combined_rate(
            &s.disk.read_bytes_per_second,
            &s.disk.write_bytes_per_second,
        ),
        combined_rate(
            &s.network.receive_bytes_per_second,
            &s.network.send_bytes_per_second,
        ),
        s.power.watts.clone(),
    ]
}
pub fn scope_for(sample: &CurrentSample, reading: &Reading<f64>) -> SeriesScope {
    SeriesScope {
        id: reading.scope_id.clone(),
        epoch: reading
            .scope_id
            .as_ref()
            .and_then(|id| sample.scopes.iter().find(|s| &s.id == id))
            .map_or(0, |s| s.epoch),
    }
}
impl ScalarFrame {
    pub fn from_sample(s: &CurrentSample, discontinuity: bool) -> Self {
        let readings = series_readings(s);
        Self {
            sequence: s.sequence,
            captured_at_ms: s.captured_at_ms,
            elapsed_ms: s.elapsed_ms,
            interval_ms: s.interval_ms,
            discontinuity,
            series: std::array::from_fn(|i| {
                let r = &readings[i];
                let valid = if i < 3 { valid_pct } else { valid_nonnegative };
                Scalar {
                    value: if r.status == ReadingStatus::Valid {
                        r.value.filter(valid)
                    } else {
                        None
                    },
                    scope: scope_for(s, r),
                }
            }),
        }
    }
}
#[derive(Clone, Debug)]
pub struct HistoryStore {
    frames: VecDeque<ScalarFrame>,
}
impl Default for HistoryStore {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(MAX_FRAMES),
        }
    }
}
impl HistoryStore {
    pub fn append(&mut self, frame: ScalarFrame) -> bool {
        if self.frames.back().is_some_and(|last| {
            frame.elapsed_ms <= last.elapsed_ms || frame.sequence <= last.sequence
        }) {
            return false;
        }
        let now = frame.elapsed_ms;
        while self.frames.len() >= MAX_FRAMES
            || self
                .frames
                .front()
                .is_some_and(|old| now.saturating_sub(old.elapsed_ms) > RETENTION_MS)
        {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
        true
    }
    pub fn frames(&self) -> &VecDeque<ScalarFrame> {
        &self.frames
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}
