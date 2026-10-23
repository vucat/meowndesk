//! US3 selection and frozen-source contract tests.
use meowndesk_lib::telemetry::{
    collector::{PollStamp, TelemetryService},
    history::HistoryStore,
    model::*,
    providers::ProviderPoll,
    state::{CommandErrorCode, ViewState},
    view,
};

fn sample() -> CurrentSample {
    static SAMPLE: std::sync::OnceLock<CurrentSample> = std::sync::OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            let json: serde_json::Value =
                serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json"))
                    .unwrap();
            serde_json::from_value(json["bootstrap"]["view"]["sample"].clone()).unwrap()
        })
        .clone()
}

fn host() -> HostIdentity {
    sample().host
}

fn stamp() -> PollStamp {
    PollStamp {
        gpu_id: None,
        gpu_epoch: 1,
        power_source: None,
        power_epoch: 1,
        interval_ms: 1000,
    }
}

fn publish(service: &TelemetryService, elapsed: u64) {
    let fixture = sample();
    service
        .publish(
            &stamp(),
            elapsed,
            1_761_091_199_000 + elapsed as i64,
            host(),
            vec![ProviderPoll {
                cpu: Some(fixture.cpu),
                memory: Some(fixture.memory),
                ..ProviderPoll::default()
            }],
        )
        .unwrap();
}

fn process(key: &str, cpu: Reading<f64>, gpu_scope: Option<&str>) -> ProcessObservation {
    ProcessObservation {
        key: key.into(),
        pid: key.split(':').next().unwrap().parse().unwrap(),
        name: key.into(),
        resident_bytes: Reading::valid(ByteCount(1_000), 1_000, 1_000, "host:system"),
        cpu_pct: cpu,
        gpu_pct: gpu_scope.map_or_else(
            || Reading {
                scope_id: None,
                ..Reading::unsupported("gpu:none", "No attribution")
            },
            |scope| Reading::valid(42.0, 1_000, 1_000, scope),
        ),
        power_w: Reading {
            scope_id: None,
            ..Reading::unsupported("power:none", "No process watts")
        },
    }
}

#[test]
fn frozen_selection_allows_ram_or_vram_but_rejects_an_unsupported_ranking() {
    let service = TelemetryService::new();
    publish(&service, 1000);
    publish(&service, 2000);
    let paused = service.set_paused(true).unwrap();
    assert_eq!(paused.mode, Mode::Paused);

    let vram = service
        .set_view(ViewSelection {
            range: Range::FiveMinutes,
            memory_view: MemoryView::Vram,
            ranking_metric: RankingMetric::Cpu,
        })
        .unwrap();
    assert_eq!(vram.selection.memory_view, MemoryView::Vram);
    assert_eq!(vram.selection.range, Range::FiveMinutes);
    assert_eq!(vram.mode, Mode::Paused);
    assert_eq!(vram.memory_breakdowns.vram, None);

    let rejected = service
        .set_view(ViewSelection {
            range: Range::FiveMinutes,
            memory_view: MemoryView::Vram,
            ranking_metric: RankingMetric::Gpu,
        })
        .unwrap_err();
    assert_eq!(rejected.code, CommandErrorCode::UnsupportedRanking);
    assert_eq!(
        service.bootstrap().view.selection.ranking_metric,
        RankingMetric::Cpu
    );
}

#[test]
fn live_ranking_loss_falls_back_to_cpu_with_a_notice() {
    let fixture = sample();
    let state = ViewState {
        ranking_metric: RankingMetric::Gpu,
        ..ViewState::default()
    };
    let view = view::build(
        &state,
        fixture,
        &HistoryStore::default(),
        &[process(
            "1:1",
            Reading::valid(25.0, 1_000, 1_000, "cpu:system"),
            None,
        )],
        CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: Some("A".into()),
        },
        vec![],
    );
    assert_eq!(view.selection.ranking_metric, RankingMetric::Cpu);
    assert!(view.notices.iter().any(|n| n.code == "ranking_fallback"));
    assert_eq!(view.process_summaries.as_ref().unwrap().gpu_top.len(), 0);
}

#[test]
fn cross_device_gpu_attribution_is_rejected() {
    let fixture = sample();
    let gpu_scope = fixture.gpu.utilization_pct.scope_id.clone().unwrap();
    let state = ViewState {
        ranking_metric: RankingMetric::Gpu,
        ..ViewState::default()
    };
    let foreign = process(
        "2:1",
        Reading::valid(5.0, 1_000, 1_000, "cpu:system"),
        Some("gpu:B"),
    );
    let view = view::build(
        &state,
        fixture,
        &HistoryStore::default(),
        &[foreign],
        CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: Some("A".into()),
        },
        vec![],
    );
    let summaries = view.process_summaries.unwrap();
    assert!(summaries.gpu_top.is_empty());
    assert!(
        !summaries
            .supported_ranking_metrics
            .contains(&RankingMetric::Gpu)
    );
    assert_eq!(view.selection.ranking_metric, RankingMetric::Cpu);
    assert_ne!(gpu_scope, "gpu:B");
}

#[test]
fn unavailable_cpu_keeps_no_fabricated_entries() {
    let fixture = sample();
    let state = ViewState::default();
    let view = view::build(
        &state,
        fixture,
        &HistoryStore::default(),
        &[process(
            "3:1",
            Reading::unavailable("cpu:system", "CPU counters are not readable"),
            None,
        )],
        CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: None,
        },
        vec![],
    );
    let summaries = view.process_summaries.unwrap();
    assert!(summaries.cpu_top.is_empty());
    assert_eq!(
        summaries.supported_ranking_metrics,
        vec![RankingMetric::Cpu]
    );
    assert_eq!(view.selection.ranking_metric, RankingMetric::Cpu);
}
