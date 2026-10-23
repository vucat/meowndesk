//! US2 history/clock/projection acceptance beyond the US1 live cases.
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
    frame_with(t, value, 1000, false)
}

fn frame_with(t: u64, value: f64, interval_ms: u64, discontinuity: bool) -> ScalarFrame {
    let mut s = sample();
    s.elapsed_ms = t;
    s.sequence = t + 1;
    s.interval_ms = interval_ms;
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
    ScalarFrame::from_sample(&s, discontinuity)
}

fn empty(endpoint: u64, range: Range) -> HistoryProjection {
    project(&HistoryStore::default(), endpoint, range, &sample())
}

#[test]
fn every_range_uses_sixty_right_closed_columns_with_exact_boundaries() {
    for range in [
        Range::OneMinute,
        Range::FiveMinutes,
        Range::FifteenMinutes,
        Range::OneHour,
    ] {
        let width = range.duration_ms();
        let endpoint = width;
        let mut history = HistoryStore::default();
        // Exactly at the window start the column is still open; +1ms and the first
        // bucket edge belong to column 0; the endpoint belongs to column 59.
        for t in [
            endpoint - width,
            endpoint - width + 1,
            endpoint - width + width / 60,
        ] {
            history.append(frame(t, 30.0));
        }
        history.append(frame(endpoint, 60.0));
        let p = project(&history, endpoint, range, &sample());
        for chart in &p.charts {
            assert_eq!(
                chart.columns.len(),
                60,
                "range {range:?} must keep 60 columns"
            );
        }
        let cpu = &p.charts[0];
        assert_eq!(cpu.columns[0].sample_count, 2);
        assert_eq!(cpu.columns[0].value, Some(30.0));
        assert_eq!(cpu.columns[59].sample_count, 1);
        assert_eq!(cpu.columns[59].value, Some(60.0));
        assert!(cpu.columns[1..59].iter().all(|c| c.value.is_none()));
        let empty = empty(endpoint, range);
        assert!(empty.charts.iter().all(|c| c.columns.len() == 60));
        assert!(
            empty
                .charts
                .iter()
                .all(|c| c.columns.iter().all(|x| x.value.is_none()))
        );
    }
}

#[test]
fn one_two_and_five_second_samples_keep_means_gaps_and_fixed_columns() {
    let mut history = HistoryStore::default();
    history.append(frame_with(55_000, 40.0, 2000, false));
    history.append(frame_with(57_000, 60.0, 2000, false));
    history.append(frame_with(59_000, 80.0, 2000, false));
    history.append(frame_with(60_000, 90.0, 1000, true));
    let p = project(&history, 60_000, Range::OneMinute, &sample());
    let cpu = &p.charts[0];
    assert_eq!(cpu.columns.len(), 60);
    assert_eq!(cpu.columns[59].value, Some(90.0));
    assert_eq!(cpu.columns[59].sample_count, 1);
    assert!(cpu.columns[59].has_gap, "a discontinuity stays a gap");
    assert_eq!(cpu.columns[58].value, Some(80.0));
    assert!(!cpu.columns[58].has_gap);
    assert!(cpu.columns[0].has_gap);
    assert!(cpu.partial_history);

    let mut five = HistoryStore::default();
    for t in (0..=60_000).step_by(5000) {
        five.append(frame_with(t, 50.0, 5000, false));
    }
    let p = project(&five, 60_000, Range::OneMinute, &sample());
    let cpu = &p.charts[0];
    // Five-second sampling leaves four empty one-second columns between samples.
    assert_eq!(cpu.columns[4].value, Some(50.0));
    assert_eq!(cpu.columns[4].sample_count, 1);
    assert_eq!(cpu.columns[5].sample_count, 0);
    assert!(cpu.columns[5].has_gap);
    assert_eq!(
        cpu.columns.iter().filter(|c| c.sample_count > 0).count(),
        12
    );
    assert!(!cpu.columns[4].has_gap);
}

#[test]
fn gpu_epoch_changes_and_counter_resets_never_mix_series_or_fabricate_rates() {
    let mut s = sample();
    s.scopes
        .iter_mut()
        .filter(|scope| scope.kind == ScopeKind::Gpu)
        .for_each(|scope| scope.epoch += 1);
    let mut history = HistoryStore::default();
    history.append(frame_with(1000, 10.0, 1000, false));
    history.append(frame_with(2000, 90.0, 1000, false));
    let p = project(&history, 300_000, Range::FiveMinutes, &sample());
    assert_eq!(p.charts[2].columns[0].value, Some(50.0));
    assert_eq!(p.gpu_sample_average_pct.value, Some(50.0));
    assert_eq!(p.gpu_sample_peak_pct.value, Some(90.0));
    let p = project(&history, 300_000, Range::FiveMinutes, &s);
    assert!(
        p.charts[2]
            .columns
            .iter()
            .all(|column| column.value.is_none()),
        "the new epoch must never be relabeled with old-GPU samples"
    );
    assert!(p.gpu_sample_peak_pct.value.is_none());
    assert_eq!(p.charts[0].columns[0].value, Some(50.0));
    s.elapsed_ms = 300_000;
    s.sequence = 300_001;
    history.append(ScalarFrame::from_sample(&s, false));
    let p = project(&history, 300_000, Range::FiveMinutes, &s);
    assert_eq!(p.charts[2].columns[59].value, Some(76.0));
    assert!(
        p.charts[2].columns[..59]
            .iter()
            .all(|column| column.value.is_none())
    );

    let mut reset = sample();
    reset.disk.read_bytes_per_second = Reading::valid(
        20.0,
        1001,
        1000,
        reset.disk.total_bytes.scope_id.clone().unwrap(),
    );
    reset.disk.write_bytes_per_second = Reading::unavailable(
        reset.disk.total_bytes.scope_id.clone().unwrap(),
        "Counter reset",
    );
    let mut history = HistoryStore::default();
    history.append(ScalarFrame::from_sample(&reset, false));
    let p = project(&history, 60_000, Range::OneMinute, &sample());
    assert_eq!(p.charts[3].columns[0].value, None);
    assert!(p.charts[3].columns[0].has_gap);
}

#[test]
fn long_runs_keep_at_most_3601_frames_across_interval_changes() {
    let mut history = HistoryStore::default();
    let mut clock = 0u64;
    for interval in [1000u64, 2000, 5000] {
        for _ in 0..2000 {
            clock += interval;
            assert!(history.append(frame_with(clock, 10.0, interval, false)));
            assert!(history.len() <= MAX_FRAMES);
        }
    }
    assert!(history.len() <= MAX_FRAMES);
    assert!(history.frames().front().unwrap().elapsed_ms >= clock - RETENTION_MS);
    // The 60 displayed columns never determine ring capacity.
    assert!(history.len() > 60);
}

#[test]
fn retention_follows_elapsed_time_when_sampling_is_slower_than_one_hertz() {
    let mut history = HistoryStore::default();
    for step in 0..=1440u64 {
        assert!(history.append(frame_with(step * 5000, 10.0, 5000, false)));
    }
    let endpoint = 7_200_000;
    assert_eq!(history.len(), 721, "one hour of five-second samples");
    assert_eq!(
        history.frames().front().unwrap().elapsed_ms,
        endpoint - RETENTION_MS
    );
    let p = project(&history, endpoint, Range::OneHour, &sample());
    assert_eq!(p.retained_from_elapsed_ms, Some(endpoint - RETENTION_MS));
    assert_eq!(p.charts[0].columns.len(), 60);
    assert!(p.charts[0].columns.iter().all(|c| c.sample_count == 12));
}

#[test]
fn time_jumps_evict_by_age_and_mark_the_next_frame_discontinuous() {
    let mut history = HistoryStore::default();
    history.append(frame_with(1000, 10.0, 1000, false));
    history.append(frame_with(3_700_000, 70.0, 1000, true));
    assert_eq!(history.len(), 1);
    assert_eq!(history.frames().front().unwrap().elapsed_ms, 3_700_000);
    let p = project(&history, 3_700_000, Range::OneHour, &sample());
    assert_eq!(p.charts[0].columns[59].value, Some(70.0));
    assert!(p.charts[0].columns[59].has_gap);
    assert_eq!(p.retained_from_elapsed_ms, Some(3_700_000));
}
