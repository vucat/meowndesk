//! US2 set_view/set_paused and delivery-race acceptance.
#[cfg(feature = "test-fixtures")]
use meowndesk_lib::telemetry::history::MAX_FRAMES;
use meowndesk_lib::telemetry::{
    collector::{PollStamp, TelemetryService},
    delivery::Sender,
    model::*,
    providers::ProviderPoll,
    state::CommandErrorCode,
};
use std::sync::{Arc, Mutex};

use tauri::Manager;

fn host() -> HostIdentity {
    let j: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json")).unwrap();
    serde_json::from_value(j["bootstrap"]["view"]["sample"]["host"].clone()).unwrap()
}

fn cpu_poll(elapsed: u64, value: f64) -> ProviderPoll {
    let at = 1_761_091_199_000 + elapsed as i64;
    let scope = "cpu:system";
    ProviderPoll {
        cpu: Some(CpuMetrics {
            total_pct: Reading::valid(value, at, elapsed, scope),
            user_pct: Reading::valid(value / 2.0, at, elapsed, scope),
            system_pct: Reading::valid(value / 4.0, at, elapsed, scope),
            temperature_c: Reading::valid(55.0, at, elapsed, scope),
        }),
        ..ProviderPoll::default()
    }
}

fn stamp(interval_ms: u64) -> PollStamp {
    PollStamp {
        gpu_id: None,
        gpu_epoch: 1,
        power_source: None,
        power_epoch: 1,
        interval_ms,
    }
}

fn publish(service: &TelemetryService, elapsed: u64, interval_ms: u64, value: f64) {
    service
        .publish(
            &stamp(interval_ms),
            elapsed,
            1_761_091_199_000 + elapsed as i64,
            host(),
            vec![cpu_poll(elapsed, value)],
        )
        .unwrap();
}

fn live_service() -> TelemetryService {
    let service = TelemetryService::new();
    for step in 1..=3u64 {
        publish(&service, step * 1000, 1000, 20.0 + step as f64);
    }
    service
}

fn selection(
    range: Range,
    memory_view: MemoryView,
    ranking_metric: RankingMetric,
) -> ViewSelection {
    ViewSelection {
        range,
        memory_view,
        ranking_metric,
    }
}

fn assert_frozen(view: &View, endpoint: u64) {
    assert_eq!(view.mode, Mode::Paused);
    assert!(view.inspection_id.is_some());
    assert_eq!(view.endpoint_elapsed_ms, endpoint);
    assert_eq!(view.sample.as_ref().unwrap().elapsed_ms, endpoint);
}

#[test]
#[cfg(feature = "test-fixtures")]
fn pause_freezes_the_endpoint_across_all_ranges_and_a_long_live_jump() {
    let service = live_service();
    let paused = service.set_paused(true).unwrap();
    assert_frozen(&paused, 3000);
    assert_eq!(paused.revision, 1);
    assert_eq!(paused.inspection_id, service.bootstrap().view.inspection_id);
    let captured = service.bootstrap().view;
    assert_eq!(
        captured.history.as_ref().unwrap().retained_from_elapsed_ms,
        Some(1000)
    );

    for range in [
        Range::OneMinute,
        Range::FiveMinutes,
        Range::FifteenMinutes,
        Range::OneHour,
    ] {
        let view = service
            .set_view(selection(range, MemoryView::Ram, RankingMetric::Cpu))
            .unwrap();
        assert_frozen(&view, 3000);
        assert_eq!(view.selection.range, range);
        let history = view.history.as_ref().unwrap();
        assert_eq!(history.range, range);
        assert_eq!(history.endpoint_elapsed_ms, 3000);
        assert!(history.charts.iter().all(|c| c.columns.len() == 60));
        assert_eq!(view.process_summaries, captured.process_summaries);
        assert_eq!(view.notices, captured.notices);
    }

    let frozen_before_jump = service.bootstrap().view;
    publish(&service, 3_700_000, 5000, 95.0);
    publish(&service, 3_705_000, 5000, 5.0);
    assert_eq!(
        serde_json::to_value(service.bootstrap().view).unwrap(),
        serde_json::to_value(&frozen_before_jump).unwrap(),
        "live collection must never roll the frozen view forward"
    );
    let (live, frozen) = service.retained_history_frames();
    assert!(live <= MAX_FRAMES);
    assert_eq!(frozen, 3, "the frozen ring survives live eviction");

    let resumed = service.set_paused(false).unwrap();
    assert_eq!(resumed.mode, Mode::Live);
    assert!(resumed.inspection_id.is_none());
    assert_eq!(resumed.endpoint_elapsed_ms, 3_705_000);
    assert_eq!(resumed.sample.as_ref().unwrap().sequence, 5);
    assert_eq!(resumed.selection.range, Range::OneHour);
    assert_eq!(
        service.retained_history_frames().1,
        0,
        "Resume releases the copy"
    );

    service.close();
    assert_eq!(service.retained_history_frames(), (0, 0));
}

#[test]
#[cfg(feature = "test-fixtures")]
fn repeated_pause_and_resume_are_idempotent_with_one_frozen_ring() {
    let service = live_service();
    let first = service.set_paused(true).unwrap();
    let second = service.set_paused(true).unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.inspection_id, second.inspection_id);
    assert_eq!(service.retained_history_frames().1, 3);

    for _ in 0..8 {
        let resumed = service.set_paused(false).unwrap();
        assert_eq!(service.retained_history_frames().1, 0);
        assert_eq!(resumed.mode, Mode::Live);
        assert_eq!(
            service.set_paused(false).unwrap().revision,
            resumed.revision
        );
        let paused = service.set_paused(true).unwrap();
        assert_eq!(paused.mode, Mode::Paused);
        let (live, frozen) = service.retained_history_frames();
        assert!(live <= MAX_FRAMES && frozen <= MAX_FRAMES);
    }
}

#[test]
fn ranking_and_range_validation_uses_the_active_source_without_spurious_revisions() {
    let service = live_service();
    let paused = service.set_paused(true).unwrap();
    let before = paused.revision;
    let error = service
        .set_view(selection(
            Range::OneMinute,
            MemoryView::Ram,
            RankingMetric::Gpu,
        ))
        .unwrap_err();
    assert_eq!(error.code, CommandErrorCode::UnsupportedRanking);
    assert_eq!(service.bootstrap().view.revision, before);

    let same = service
        .set_view(selection(
            Range::OneMinute,
            MemoryView::Ram,
            RankingMetric::Cpu,
        ))
        .unwrap();
    assert_eq!(
        same.revision, before,
        "a no-op selection retains the revision"
    );

    let changed = service
        .set_view(selection(
            Range::FiveMinutes,
            MemoryView::Ram,
            RankingMetric::Cpu,
        ))
        .unwrap();
    assert_eq!(changed.revision, before + 1);
    assert_frozen(&changed, 3000);
}

#[test]
fn pause_clears_pending_live_delivery_and_resume_never_replays_missed_updates() {
    let delivered: Arc<Mutex<Vec<Delivery>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = delivered.clone();
    let sender: Sender = Arc::new(move |delivery| {
        sink.lock().unwrap().push(delivery);
        Ok(())
    });
    let service = live_service();
    let (id, _) = service.subscribe(sender).unwrap();
    service.ack(&id, 0).unwrap();
    assert_eq!(service.counts(), (1, 0, 0));

    publish(&service, 4000, 1000, 30.0);
    assert_eq!(
        service.counts(),
        (1, 1, 0),
        "one in-flight plus one pending"
    );
    service.set_paused(true).unwrap();
    assert_eq!(
        service.counts(),
        (1, 0, 0),
        "Pause clears the pending live send"
    );
    assert_eq!(delivered.lock().unwrap().len(), 1);

    service.ack(&id, 1).unwrap();
    assert_eq!(
        delivered.lock().unwrap().len(),
        1,
        "no queued update is replayed"
    );
    service.set_paused(false).unwrap();
    assert_eq!(
        delivered.lock().unwrap().len(),
        1,
        "Resume returns data with its response"
    );

    publish(&service, 4_100_000, 5000, 80.0);
    let deliveries = delivered.lock().unwrap();
    assert_eq!(deliveries.len(), 2);
    let latest = &deliveries[1];
    assert_eq!(latest.delivery_sequence, 2);
    assert_eq!(latest.view.mode, Mode::Live);
    assert_eq!(latest.view.endpoint_elapsed_ms, 4_100_000);
    assert_eq!(latest.view.sample.as_ref().unwrap().sequence, 5);
}

#[test]
fn hidden_windows_suppress_delivery_in_both_modes_and_restore_the_cached_source() {
    let delivered: Arc<Mutex<Vec<Delivery>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = delivered.clone();
    let sender: Sender = Arc::new(move |delivery| {
        sink.lock().unwrap().push(delivery);
        Ok(())
    });
    let service = live_service();
    let (id, _) = service.subscribe(sender).unwrap();
    service.ack(&id, 0).unwrap();
    publish(&service, 4000, 1000, 30.0);
    assert_eq!(service.counts(), (1, 1, 0));

    let hidden = service.visible(false);
    assert!(!hidden.visible);
    assert_eq!(service.counts(), (1, 0, 0));
    service.ack(&id, 1).unwrap();
    assert_eq!(delivered.lock().unwrap().len(), 1);
    let restored = service.visible(true);
    assert!(restored.visible);
    assert_eq!(restored.endpoint_elapsed_ms, 4000);

    let paused = service.set_paused(true).unwrap();
    assert_eq!(paused.endpoint_elapsed_ms, 4000);
    service.visible(false);
    publish(&service, 5000, 1000, 70.0);
    let hidden = service.bootstrap().view;
    assert!(
        !hidden.visible,
        "hide is reported while the paused source stays displayed"
    );
    assert_eq!(hidden.mode, Mode::Paused);
    assert_eq!(hidden.endpoint_elapsed_ms, 4000);
    assert_eq!(hidden.sample.as_ref().unwrap().sequence, 4);
    assert_eq!(
        hidden.inspection_id, paused.inspection_id,
        "hide preserves the frozen inspection"
    );
    let restored = service.visible(true);
    assert_eq!(restored.mode, Mode::Paused);
    assert!(restored.visible);
    assert_eq!(restored.endpoint_elapsed_ms, 4000);
    assert_eq!(restored.sample.as_ref().unwrap().sequence, 4);
}

#[test]
fn ipc_commands_validate_selection_and_pause_through_the_real_acl() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = meowndesk_lib::configure(tauri::test::mock_builder())
        .build(context)
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let service = app.state::<Arc<TelemetryService>>().inner().clone();
    publish(&service, 1000, 1000, 42.0);
    let invoke = |command: &str, args: serde_json::Value| -> serde_json::Value {
        tauri::test::get_ipc_response(
            &window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: window.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .unwrap()
        .deserialize()
        .unwrap()
    };
    let paused = invoke("set_paused", serde_json::json!({"paused": true}));
    assert_eq!(paused["ok"], true);
    assert_eq!(paused["data"]["view"]["mode"], "paused");
    assert!(paused["data"]["view"]["inspectionId"].is_string());

    let invalid = invoke(
        "set_view",
        serde_json::json!({"selection":{"range":"2m","memoryView":"ram","rankingMetric":"cpu"}}),
    );
    assert_eq!(invalid["ok"], false);
    assert_eq!(invalid["error"]["code"], "invalid_argument");

    let selected = invoke(
        "set_view",
        serde_json::json!({"selection":{"range":"15m","memoryView":"ram","rankingMetric":"cpu"}}),
    );
    assert_eq!(selected["ok"], true);
    assert_eq!(selected["data"]["view"]["mode"], "paused");
    assert_eq!(selected["data"]["view"]["selection"]["range"], "15m");

    let resumed = invoke("set_paused", serde_json::json!({"paused": false}));
    assert_eq!(resumed["data"]["view"]["mode"], "live");
    assert!(resumed["data"]["view"]["inspectionId"].is_null());
}

#[test]
fn frozen_source_keeps_its_baseline_notices_and_bounded_lists() {
    let service = live_service();
    let frozen = service.set_paused(true).unwrap();
    publish(&service, 4000, 1000, 90.0);
    publish(&service, 5000, 1000, 10.0);
    publish(&service, 3_700_000, 5000, 50.0);
    let current = service.bootstrap().view;
    assert_eq!(current.notices, frozen.notices);
    assert_eq!(current.process_summaries, frozen.process_summaries);
    assert_eq!(current.memory_breakdowns, frozen.memory_breakdowns);
    let reprojected = service
        .set_view(selection(
            Range::OneHour,
            MemoryView::Ram,
            RankingMetric::Cpu,
        ))
        .unwrap();
    assert_eq!(reprojected.notices, frozen.notices);
    assert_eq!(reprojected.endpoint_at_ms, frozen.endpoint_at_ms);
}
