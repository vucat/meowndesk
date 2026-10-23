//! Pure right-closed, 60-column projections from recorded observations. Never interpolate gaps.
use crate::telemetry::{history::*, model::*};
pub fn nice_scale(max: f64) -> f64 {
    if !max.is_finite() || max <= 1.0 {
        return 1.0;
    }
    let order = 10_f64.powf(max.log10().floor());
    for step in [1.0, 2.0, 5.0, 10.0] {
        let scale = step * order;
        if scale >= max {
            return if scale.is_finite() { scale } else { f64::MAX };
        }
    }
    f64::MAX
}
pub fn project(
    history: &HistoryStore,
    endpoint: u64,
    range: Range,
    sample: &CurrentSample,
) -> HistoryProjection {
    let start = i128::from(endpoint) - i128::from(range.duration_ms());
    let readings = series_readings(sample);
    let mut charts = Vec::with_capacity(6);
    let mut gpu_values = Vec::new();
    let mut gpu_last = None;
    for i in 0..6 {
        let expected = scope_for(sample, &readings[i]);
        let mut means = [0.0; 60];
        let mut counts = [0u32; 60];
        let mut gaps = [false; 60];
        for frame in history.frames() {
            let relative = i128::from(frame.elapsed_ms) - start;
            if relative <= 0 || frame.elapsed_ms > endpoint {
                continue;
            }
            let bucket = ((relative * 60 - 1) / i128::from(range.duration_ms())) as usize;
            if bucket >= 60 {
                continue;
            }
            let scalar = &frame.series[i];
            if scalar.scope == expected
                && let Some(value) = scalar.value
            {
                counts[bucket] += 1;
                means[bucket] += (value - means[bucket]) / f64::from(counts[bucket]);
                if i == 2 {
                    gpu_values.push(value);
                    gpu_last = Some((frame.captured_at_ms, frame.elapsed_ms));
                }
            } else {
                gaps[bucket] = true;
            }
            if frame.discontinuity {
                gaps[bucket] = true;
            }
        }
        let columns: Vec<_> = (0..60)
            .map(|b| ChartColumn {
                value: (counts[b] > 0).then_some(means[b]),
                sample_count: counts[b],
                has_gap: gaps[b] || counts[b] == 0,
            })
            .collect();
        let any = counts.iter().any(|n| *n > 0);
        let max = means.iter().copied().fold(0.0, f64::max);
        // Chart status describes whether the selected window contains usable samples.
        // Individual current readings retain unsupported/denied reasons independently.
        let status = if any {
            ReadingStatus::Valid
        } else {
            ReadingStatus::Unavailable
        };
        let scope_label = expected
            .id
            .as_ref()
            .and_then(|id| sample.scopes.iter().find(|s| &s.id == id))
            .map(|s| s.label.clone());
        let partial = history
            .frames()
            .front()
            .is_none_or(|f| i128::from(f.elapsed_ms) > start)
            || columns.iter().any(|c| c.has_gap);
        charts.push(Chart {
            series_id: SERIES[i],
            unit: SERIES[i].expect_unit(),
            scope_id: expected.id,
            scope_label,
            scale_max: if i < 3 { 100.0 } else { nice_scale(max) },
            columns,
            partial_history: partial,
            status,
        });
    }
    let stats = |value: Option<f64>| match (value, gpu_last) {
        (Some(value), Some((at, elapsed))) => Reading::valid(
            value,
            at,
            elapsed,
            readings[2].scope_id.as_deref().unwrap_or("gpu:unavailable"),
        ),
        _ => Reading {
            scope_id: readings[2].scope_id.clone(),
            ..Reading::unavailable(
                "gpu:unavailable",
                "No valid samples for this GPU and time range",
            )
        },
    };
    let average =
        (!gpu_values.is_empty()).then(|| gpu_values.iter().sum::<f64>() / gpu_values.len() as f64);
    let peak = gpu_values.into_iter().reduce(f64::max);
    HistoryProjection {
        range,
        endpoint_elapsed_ms: endpoint,
        retained_from_elapsed_ms: history.frames().front().map(|f| f.elapsed_ms.min(endpoint)),
        charts,
        gpu_sample_average_pct: stats(average),
        gpu_sample_peak_pct: stats(peak),
    }
}
