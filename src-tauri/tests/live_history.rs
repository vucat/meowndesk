//! US1 live history/projection acceptance, written before T034/T035 implementation.
use meowndesk_lib::telemetry::{history::*, model::*, projection::*};
fn sample() -> CurrentSample {
    static SAMPLE: std::sync::OnceLock<CurrentSample> = std::sync::OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            let j: serde_json::Value =
                serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json"))
                    .unwrap();
            serde_json::from_value(j["bootstrap"]["view"]["sample"].clone()).unwrap()
        })
        .clone()
}
fn frame(t: u64, value: f64) -> ScalarFrame {
    let mut s = sample();
    s.elapsed_ms = t;
    s.sequence = t + 1;
    s.cpu.total_pct = Reading::valid(
        value,
        1000 + t as i64,
        t,
        s.cpu.total_pct.scope_id.clone().unwrap(),
    );
    s.gpu.utilization_pct = Reading::valid(
        value,
        1000 + t as i64,
        t,
        s.gpu.utilization_pct.scope_id.clone().unwrap(),
    );
    ScalarFrame::from_sample(&s, false)
}
#[test]
fn ring_evicts_by_age_and_capacity_and_rejects_reordered_frames() {
    let mut history = HistoryStore::default();
    for t in 0..5000 {
        assert!(history.append(frame(t * 1000, 0.0)));
        assert!(history.len() <= 3601);
    }
    assert_eq!(history.len(), 3601);
    assert_eq!(history.frames().front().unwrap().elapsed_ms, 1_399_000);
    assert!(!history.append(frame(1000, 1.0)));
    history.append(frame(9_000_000, 1.0));
    assert_eq!(history.len(), 1);
}
#[test]
fn right_closed_columns_keep_real_zero_and_empty_buckets_distinct() {
    let s = sample();
    let mut history = HistoryStore::default();
    history.append(frame(0, 99.0));
    history.append(frame(1000, 0.0));
    history.append(frame(2000, 50.0));
    let p = project(&history, 60_000, Range::OneMinute, &s);
    assert_eq!(p.charts.len(), 6);
    assert!(p.charts.iter().all(|c| c.columns.len() == 60));
    let cpu = &p.charts[0];
    assert_eq!(cpu.columns[0].value, Some(0.0));
    assert_eq!(cpu.columns[1].value, Some(50.0));
    assert_eq!(cpu.columns[2].value, None);
    assert!(!cpu.columns[0].has_gap);
    assert!(cpu.columns[2].has_gap);
    assert_eq!(cpu.scale_max, 100.0);
}
#[test]
fn raw_gpu_peak_is_not_the_peak_of_bucket_means_and_epochs_never_mix() {
    let mut s = sample();
    let mut history = HistoryStore::default();
    history.append(frame(1000, 10.0));
    history.append(frame(2000, 90.0));
    let p = project(&history, 300_000, Range::FiveMinutes, &s);
    assert_eq!(p.charts[2].columns[0].value, Some(50.0));
    assert_eq!(p.gpu_sample_peak_pct.value, Some(90.0));
    assert_eq!(p.gpu_sample_average_pct.value, Some(50.0));
    s.scopes
        .iter_mut()
        .filter(|s| s.kind == ScopeKind::Gpu)
        .for_each(|s| s.epoch += 1);
    let p = project(&history, 300_000, Range::FiveMinutes, &s);
    assert!(p.charts[2].columns.iter().all(|c| c.value.is_none()));
    assert!(p.gpu_sample_peak_pct.value.is_none());
    assert_eq!(p.charts[0].columns[0].value, Some(50.0));
}
#[test]
fn stale_and_partial_component_rates_are_gaps_and_scale_uses_nice_steps() {
    let mut s = sample();
    s.cpu.total_pct.status = ReadingStatus::Stale;
    s.cpu.total_pct.reason = Some("No fresh sample".into());
    s.disk.read_bytes_per_second =
        Reading::valid(20.0, 1, 1, s.disk.total_bytes.scope_id.clone().unwrap());
    s.disk.write_bytes_per_second =
        Reading::unavailable(s.disk.total_bytes.scope_id.clone().unwrap(), "Reset");
    let frame = ScalarFrame::from_sample(&s, false);
    assert!(frame.series[0].value.is_none());
    assert!(frame.series[3].value.is_none());
    assert_eq!(nice_scale(0.0), 1.0);
    assert_eq!(nice_scale(2.1), 5.0);
    assert_eq!(nice_scale(1001.0), 2000.0);
}
