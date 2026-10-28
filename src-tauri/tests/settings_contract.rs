//! US4 interval/GPU command contract, persistence outcomes and delayed-poll race.
use meowndesk_lib::{
    preferences,
    telemetry::{
        collector::{PollStamp, TelemetryService},
        model::*,
        normalize::empty_gpu,
        providers::ProviderPoll,
        scopes::{GpuCandidate, GpuIdentityKind},
        state::CommandErrorCode,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "meowndesk-settings-{name}-{}-{unique}",
        std::process::id()
    ))
}
fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

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

fn candidate(id: &str, name: &str) -> GpuCandidate {
    GpuCandidate::new(
        id,
        name,
        GpuVendor::Nvidia,
        GpuKind::Discrete,
        true,
        GpuIdentityKind::NvmlUuid,
    )
}

fn gpu_poll(elapsed: u64, scope: &str) -> ProviderPoll {
    let fixture = sample();
    let mut gpu = empty_gpu(scope);
    let at = 1_761_091_199_000 + elapsed as i64;
    gpu.utilization_pct = Reading::valid(50.0, at, elapsed, scope);
    gpu.vram_total_bytes = Reading::valid(ByteCount(8_000), at, elapsed, scope);
    gpu.vram_used_bytes = Reading::valid(ByteCount(2_000), at, elapsed, scope);
    gpu.vram_free_bytes = Reading::valid(ByteCount(6_000), at, elapsed, scope);
    ProviderPoll {
        cpu: Some(fixture.cpu),
        memory: Some(fixture.memory),
        gpu: Some(gpu),
        processes: vec![ProcessObservation {
            key: "1:1".into(),
            pid: 1,
            name: "workload".into(),
            resident_bytes: Reading::valid(ByteCount(1_000), at, elapsed, "host:alex-workstation"),
            cpu_pct: Reading::valid(5.0, at, elapsed, "cpu:package0"),
            gpu_pct: Reading::valid(40.0, at, elapsed, scope),
            power_w: Reading::unsupported(scope, "No process watts"),
        }],
        ..ProviderPoll::default()
    }
}

fn publish(service: &TelemetryService, stamp: &PollStamp, elapsed: u64, scope: &str) {
    service
        .publish(
            stamp,
            elapsed,
            1_761_091_199_000 + elapsed as i64,
            host(),
            vec![gpu_poll(elapsed, scope)],
        )
        .unwrap();
}

#[test]
fn interval_changes_persist_validate_and_no_ops_retain_revision() {
    let directory = temp_path("interval");
    let path = directory.join(preferences::FILE_NAME);
    let service = TelemetryService::new();
    service.initialize_preferences(path.clone(), preferences::defaults(), None);

    let (view, persisted) = service.set_preferences(2000, None).unwrap();
    assert!(persisted);
    assert_eq!(view.preferences.interval_ms, 2000);
    assert_eq!(service.capture_poll().interval_ms, 2000);
    assert_eq!(
        preferences::load(&path).preferences.interval_ms,
        2000,
        "the atomic write is readable"
    );
    let revision = view.revision;
    let (same, persisted) = service.set_preferences(2000, None).unwrap();
    assert!(persisted);
    assert_eq!(same.revision, revision);

    let error = service.set_preferences(3000, None).unwrap_err();
    assert_eq!(error.code, CommandErrorCode::InvalidArgument);
    assert_eq!(service.bootstrap().view.preferences.interval_ms, 2000);
    cleanup(&directory);
}

#[cfg(unix)]
#[test]
fn failed_replacement_keeps_old_disk_preferences_and_new_in_session_settings() {
    use std::os::unix::fs::PermissionsExt;
    let directory = temp_path("replace-failure");
    let path = directory.join(preferences::FILE_NAME);
    preferences::save(&path, &preferences::defaults()).unwrap();
    let original = fs::read(&path).unwrap();
    let service = TelemetryService::new();
    service.initialize_preferences(path.clone(), preferences::defaults(), None);
    let temporary = path.with_file_name("preferences.json.tmp");
    fs::write(&temporary, b"temporary").unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o444)).unwrap();
    let (view, persisted) = service.set_preferences(5000, None).unwrap();
    assert!(!persisted);
    assert_eq!(view.preferences.interval_ms, 5000);
    assert_eq!(service.capture_poll().interval_ms, 5000);
    assert!(
        view.notices
            .iter()
            .any(|notice| notice.code == "preferences_not_saved")
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let temporary_remains = temporary.exists();
    cleanup(&directory);
    assert!(!temporary_remains);
}

#[test]
fn selected_gpu_persists_and_unknown_or_paused_changes_are_rejected() {
    let directory = temp_path("gpu");
    let path = directory.join(preferences::FILE_NAME);
    let service = TelemetryService::new();
    service.initialize_preferences(path.clone(), preferences::defaults(), None);
    service.reconcile_gpus(vec![
        candidate("GPU-A", "Adapter A"),
        candidate("GPU-B", "Adapter B"),
    ]);
    let (view, persisted) = service.set_preferences(1000, Some("GPU-B".into())).unwrap();
    assert!(persisted);
    assert_eq!(view.preferences.selected_gpu_id.as_deref(), Some("GPU-B"));
    assert_eq!(
        preferences::load(&path)
            .preferences
            .selected_gpu_id
            .as_deref(),
        Some("GPU-B")
    );
    let error = service
        .set_preferences(1000, Some("GPU-missing".into()))
        .unwrap_err();
    assert_eq!(error.code, CommandErrorCode::UnknownGpu);

    let stamp = service.capture_poll();
    publish(&service, &stamp, 1000, "gpu:GPU-B");
    let paused = service.set_paused(true).unwrap();
    let frozen_endpoint = paused.endpoint_elapsed_ms;
    let revision = paused.revision;
    let error = service
        .set_preferences(1000, Some("GPU-A".into()))
        .unwrap_err();
    assert_eq!(error.code, CommandErrorCode::ResumeRequired);
    let (same, persisted) = service.set_preferences(5000, Some("GPU-B".into())).unwrap();
    assert!(persisted);
    assert_eq!(same.revision, revision + 1);
    assert_eq!(same.preferences.interval_ms, 5000);
    assert_eq!(same.endpoint_elapsed_ms, frozen_endpoint);
    assert_eq!(same.mode, Mode::Paused);
    cleanup(&directory);
}

#[test]
fn a_failed_write_keeps_in_session_choices_and_reports_persistence_failure() {
    let directory = temp_path("failure");
    fs::create_dir_all(&directory).unwrap();
    let blocker = directory.join("blocked");
    fs::write(&blocker, b"file").unwrap();
    let service = TelemetryService::new();
    service.initialize_preferences(
        blocker.join(preferences::FILE_NAME),
        preferences::defaults(),
        None,
    );
    let (view, persisted) = service.set_preferences(5000, None).unwrap();
    assert!(!persisted);
    assert_eq!(view.preferences.interval_ms, 5000);
    assert!(
        view.notices
            .iter()
            .any(|notice| notice.code == "preferences_not_saved")
    );
    cleanup(&directory);
}

#[test]
fn saved_device_fallback_records_a_notice_and_does_not_reuse_old_identity() {
    let directory = temp_path("fallback");
    let service = TelemetryService::new();
    service.initialize_preferences(
        directory.join(preferences::FILE_NAME),
        CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: Some("GPU-missing".into()),
        },
        None,
    );
    service.reconcile_gpus(vec![candidate("GPU-A", "Adapter A")]);
    let view = service.bootstrap().view;
    assert_eq!(view.preferences.selected_gpu_id.as_deref(), Some("GPU-A"));
    assert!(
        view.notices
            .iter()
            .any(|notice| notice.code == "gpu_fallback")
    );
    cleanup(&directory);
}

#[test]
fn startup_notices_survive_samples_before_the_first_subscription() {
    use std::sync::{Arc, Mutex};
    let directory = temp_path("startup-notices");
    let service = TelemetryService::new();
    service.initialize_preferences(
        directory.join(preferences::FILE_NAME),
        CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: Some("GPU-missing".into()),
        },
        Some("Saved preferences were reset to defaults".into()),
    );
    service.reconcile_gpus(vec![candidate("GPU-A", "Adapter A")]);
    for elapsed in [64_000, 65_000, 66_000] {
        publish(&service, &service.capture_poll(), elapsed, "gpu:GPU-A");
    }
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    let (id, _) = service
        .subscribe(Arc::new(move |delivery| {
            received.lock().unwrap().push(delivery);
            Ok(())
        }))
        .unwrap();
    assert!(service.ack(&id, 0).unwrap());
    let deliveries = messages.lock().unwrap();
    for code in ["preferences_reset", "gpu_fallback"] {
        assert_eq!(
            deliveries[0]
                .view
                .notices
                .iter()
                .filter(|n| n.code == code)
                .count(),
            1,
            "startup notice {code} must reach the first subscriber exactly once per view"
        );
    }
    drop(deliveries);
    let (_, persisted) = service.set_preferences(1000, Some("GPU-A".into())).unwrap();
    assert!(persisted);
    publish(&service, &service.capture_poll(), 67_000, "gpu:GPU-A");
    assert!(!service.bootstrap().view.notices.iter().any(|n| {
        matches!(
            n.code.as_str(),
            "preferences_reset" | "gpu_fallback" | "preferences_not_saved"
        )
    }));
    cleanup(&directory);
}

#[test]
fn save_failure_survives_publication_and_paused_controls_until_a_successful_save() {
    let directory = temp_path("notice-recovery");
    fs::create_dir_all(&directory).unwrap();
    let blocker = directory.join("blocked");
    fs::write(&blocker, b"file").unwrap();
    let service = TelemetryService::new();
    service.initialize_preferences(
        blocker.join(preferences::FILE_NAME),
        preferences::defaults(),
        None,
    );
    publish(&service, &service.capture_poll(), 64_000, "gpu:unselected");
    let (_, persisted) = service.set_preferences(2000, None).unwrap();
    assert!(!persisted);
    publish(&service, &service.capture_poll(), 66_000, "gpu:unselected");
    let paused = service.set_paused(true).unwrap();
    assert!(
        paused
            .notices
            .iter()
            .any(|n| n.code == "preferences_not_saved")
    );
    publish(&service, &service.capture_poll(), 68_000, "gpu:unselected");
    let ranged = service
        .set_view(ViewSelection {
            range: Range::FifteenMinutes,
            ..paused.selection
        })
        .unwrap();
    assert_eq!(ranged.sample, paused.sample);
    assert!(
        ranged
            .notices
            .iter()
            .any(|n| n.code == "preferences_not_saved")
    );
    fs::remove_file(&blocker).unwrap();
    let (saved, persisted) = service.set_preferences(2000, None).unwrap();
    assert!(persisted);
    assert!(
        !saved
            .notices
            .iter()
            .any(|n| n.code == "preferences_not_saved")
    );
    let resumed = service.set_paused(false).unwrap();
    assert!(
        !resumed
            .notices
            .iter()
            .any(|n| n.code == "preferences_not_saved")
    );
    cleanup(&directory);
}

#[test]
fn a_late_poll_for_the_previous_gpu_cannot_repopulate_the_new_selection() {
    let directory = temp_path("race");
    let service = TelemetryService::new();
    service.initialize_preferences(
        directory.join(preferences::FILE_NAME),
        preferences::defaults(),
        None,
    );
    service.reconcile_gpus(vec![
        candidate("GPU-A", "Adapter A"),
        candidate("GPU-B", "Adapter B"),
    ]);
    service.set_preferences(1000, Some("GPU-A".into())).unwrap();
    let stale_stamp = service.capture_poll();
    service.set_preferences(1000, Some("GPU-B".into())).unwrap();

    publish(&service, &stale_stamp, 1000, "gpu:GPU-A");
    let late = service.bootstrap().view;
    assert_eq!(late.preferences.selected_gpu_id.as_deref(), Some("GPU-B"));
    assert_eq!(
        late.sample.as_ref().unwrap().gpu.utilization_pct.status,
        ReadingStatus::Unavailable
    );
    assert_eq!(
        late.sample.as_ref().unwrap().cpu.total_pct.status,
        ReadingStatus::Valid
    );
    assert!(
        late.process_summaries
            .as_ref()
            .is_some_and(|summaries| summaries.gpu_top.is_empty())
    );

    let fresh_stamp = service.capture_poll();
    publish(&service, &fresh_stamp, 2000, "gpu:GPU-B");
    let fresh = service.bootstrap().view;
    assert_eq!(
        fresh.sample.as_ref().unwrap().gpu.utilization_pct.status,
        ReadingStatus::Valid
    );
    assert_eq!(
        fresh.sample.as_ref().unwrap().gpu.utilization_pct.value,
        Some(50.0)
    );
    assert_eq!(
        fresh
            .process_summaries
            .as_ref()
            .map(|summaries| summaries.gpu_top.len()),
        Some(1)
    );
    cleanup(&directory);
}

#[test]
fn restart_with_a_saved_gpu_has_a_valid_bootstrap_before_discovery_and_restores_it_after() {
    let directory = temp_path("startup");
    let service = TelemetryService::new();
    service.initialize_preferences(
        directory.join(preferences::FILE_NAME),
        CollectionPreferences {
            interval_ms: 2000,
            selected_gpu_id: Some("GPU-B".into()),
        },
        None,
    );
    let initializing = service.bootstrap();
    initializing.validate().unwrap();
    assert!(initializing.gpus.is_empty());
    assert_eq!(initializing.view.preferences.selected_gpu_id, None);
    assert_eq!(initializing.view.preferences.interval_ms, 2000);
    service.reconcile_gpus(vec![
        candidate("GPU-A", "Adapter A"),
        candidate("GPU-B", "Adapter B"),
    ]);
    let discovered = service.bootstrap();
    discovered.validate().unwrap();
    assert_eq!(
        discovered.view.preferences.selected_gpu_id.as_deref(),
        Some("GPU-B")
    );
    assert!(
        !discovered
            .view
            .notices
            .iter()
            .any(|n| n.code == "gpu_fallback")
    );
    cleanup(&directory);
}
