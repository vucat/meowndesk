//! Bounded process ranking. CPU values arrive already normalized to whole-machine percent.
use crate::telemetry::model::*;
use std::collections::BTreeMap;

fn valid(reading: &Reading<f64>) -> bool {
    reading.status == ReadingStatus::Valid && reading.value.is_some()
}

// ASCII case folding and suffix removal handle Windows names and Linux executables alike.
fn executable_name(name: &str) -> &str {
    name.get(..name.len().saturating_sub(4))
        .filter(|_| {
            name.get(name.len().saturating_sub(4)..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
        })
        .unwrap_or(name)
}

// The same canonical key drives grouping and friendly labels on both platforms.
fn normalized_executable_name(name: &str) -> String {
    let normalized = executable_name(name).to_ascii_lowercase();
    match normalized.as_str() {
        "vmmemwsl" => "vmmem".into(),
        _ => normalized,
    }
}

fn app_name(name: &str) -> String {
    let label = match normalized_executable_name(name).as_str() {
        "vmmem" => "WSL2",
        "chrome" => "Chrome",
        "code" => "VS Code",
        "msedge" => "Edge",
        "explorer" => "Explorer",
        "opencode" => "OpenCode",
        "docker" => "Docker",
        "ollama" => "Ollama",
        "sharex" => "ShareX",
        _ => executable_name(name),
    };
    label.into()
}

// Borrow current observations while summing; clone only the bounded winning rows.
// A missing value after the first source means overflow/incompatible scope, never a partial sum.
struct Total<'a, T> {
    source: Option<&'a Reading<T>>,
    value: Option<T>,
}
impl<'a, T: Copy> Total<'a, T> {
    fn new() -> Self {
        Self {
            source: None,
            value: None,
        }
    }
    fn add(&mut self, reading: &'a Reading<T>, sum: impl Fn(T, T) -> Option<T>) {
        if reading.status != ReadingStatus::Valid || reading.value.is_none() {
            return;
        }
        if let Some(source) = self.source {
            self.value = self.value.zip(reading.value).and_then(|(a, b)| {
                (source.scope_id == reading.scope_id)
                    .then(|| sum(a, b))
                    .flatten()
            });
        } else {
            self.source = Some(reading);
            self.value = reading.value;
        }
    }
    fn reading(&self, fallback: &Reading<T>) -> Reading<T> {
        match (self.source, self.value) {
            (Some(source), Some(value)) => Reading {
                value: Some(value),
                ..source.clone()
            },
            (Some(source), None) => Reading {
                scope_id: source.scope_id.clone(),
                ..Reading::unavailable("app", "App total overflow or incompatible reading scopes")
            },
            (None, _) => fallback.clone(),
        }
    }
}

struct App<'a> {
    representative: &'a ProcessObservation,
    memory: Total<'a, ByteCount>,
    cpu: Total<'a, f64>,
    count: usize,
}
impl App<'_> {
    fn observation(&self) -> ProcessObservation {
        let mut row = self.representative.clone();
        row.name = app_name(&row.name);
        row.resident_bytes = self.memory.reading(&row.resident_bytes);
        row.cpu_pct = self.cpu.reading(&row.cpu_pct);
        if self.count > 1 {
            // GPU/power stay per PID in their own rankings; do not mislabel one PID as an app total.
            row.gpu_pct = Reading {
                scope_id: row.gpu_pct.scope_id,
                ..Reading::unavailable("app", "Per-app GPU attribution is unavailable")
            };
            row.power_w = Reading {
                scope_id: row.power_w.scope_id,
                ..Reading::unavailable("app", "Per-app power attribution is unavailable")
            };
        }
        row
    }
}

fn top<'a, T>(
    processes: impl IntoIterator<Item = &'a T>,
    limit: usize,
    include: impl Fn(&T) -> bool,
    compare: impl Fn(&T, &T) -> std::cmp::Ordering,
) -> Vec<&'a T> {
    let mut rows: Vec<&T> = Vec::with_capacity(limit + 1);
    for process in processes.into_iter().filter(|process| include(process)) {
        let at = rows.partition_point(|old| compare(old, process).is_le());
        if at < limit {
            rows.insert(at, process);
            rows.truncate(limit);
        }
    }
    rows
}

/// Aggregate the complete current process table before taking eight memory/five CPU apps.
/// Each group carries its lexicographically smallest live process key/PID as representative.
pub fn summarize(processes: &[ProcessObservation], gpu_scope: Option<&str>) -> ProcessSummaries {
    let mut apps = BTreeMap::new();
    for process in processes {
        let normalized = normalized_executable_name(&process.name);
        let app = apps.entry(normalized).or_insert_with(|| App {
            representative: process,
            memory: Total::new(),
            cpu: Total::new(),
            count: 0,
        });
        if process.key < app.representative.key {
            app.representative = process;
        }
        app.count += 1;
        app.memory.add(&process.resident_bytes, |a, b| {
            a.0.checked_add(b.0).map(ByteCount)
        });
        app.cpu.add(&process.cpu_pct, |a, b| {
            let total = a + b;
            (total.is_finite() && (0.0..=100.0 + 1e-9).contains(&total)).then_some(total.min(100.0))
        });
    }
    let memory = top(
        apps.values(),
        MAX_PROCESS_MEMORY_ENTRIES,
        |app| app.memory.value.is_some(),
        |a, b| {
            b.memory
                .value
                .cmp(&a.memory.value)
                .then(a.representative.key.cmp(&b.representative.key))
        },
    )
    .into_iter()
    .map(App::observation)
    .collect();
    let cpu = top(
        apps.values(),
        MAX_PROCESS_RANK_ENTRIES,
        |app| app.cpu.value.is_some(),
        |a, b| {
            b.cpu
                .value
                .unwrap_or_default()
                .total_cmp(&a.cpu.value.unwrap_or_default())
                .then(a.representative.key.cmp(&b.representative.key))
        },
    )
    .into_iter()
    .map(App::observation)
    .collect();
    let same_gpu = |process: &ProcessObservation| {
        gpu_scope.is_some()
            && process.gpu_pct.scope_id.as_deref() == gpu_scope
            && valid(&process.gpu_pct)
    };
    let gpu = top(
        processes.iter(),
        MAX_PROCESS_RANK_ENTRIES,
        same_gpu,
        |a, b| {
            b.gpu_pct
                .value
                .unwrap_or_default()
                .total_cmp(&a.gpu_pct.value.unwrap_or_default())
                .then(a.key.cmp(&b.key))
        },
    )
    .into_iter()
    .cloned()
    .collect::<Vec<_>>();
    let power = top(
        processes.iter(),
        MAX_PROCESS_RANK_ENTRIES,
        |process| valid(&process.power_w),
        |a, b| {
            b.power_w
                .value
                .unwrap_or_default()
                .total_cmp(&a.power_w.value.unwrap_or_default())
                .then(a.key.cmp(&b.key))
        },
    )
    .into_iter()
    .cloned()
    .collect::<Vec<_>>();

    let mut memory: Vec<ProcessObservation> = memory;
    let mut total = 0u64;
    memory.retain(|process| {
        if let Some(next) = process
            .resident_bytes
            .value
            .and_then(|bytes| total.checked_add(bytes.0))
        {
            total = next;
            true
        } else {
            false
        }
    });

    let mut supported_ranking_metrics = vec![RankingMetric::Cpu];
    if !gpu.is_empty() {
        supported_ranking_metrics.push(RankingMetric::Gpu);
    }
    if !power.is_empty() {
        supported_ranking_metrics.push(RankingMetric::Power);
    }
    let mut coverage_notes = vec![
        "Resident pages may be shared; displayed processes are not a partition of system RAM"
            .into(),
        "Processes sharing an executable name are shown as one app".into(),
    ];
    if gpu_scope.is_some() && gpu.is_empty() {
        coverage_notes
            .push("Per-process GPU attribution is unavailable for the selected adapter".into());
    }
    if power.is_empty() {
        coverage_notes.push("Measured per-process power is unavailable".into());
    }
    ProcessSummaries {
        memory_top: memory,
        cpu_top: cpu,
        gpu_top: gpu,
        power_top: power,
        supported_ranking_metrics,
        coverage_notes,
        displayed_memory_total_bytes: ByteCount(total),
    }
}
