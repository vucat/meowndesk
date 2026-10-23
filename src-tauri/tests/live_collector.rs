#![cfg(feature = "test-fixtures")]
//! Exercise the actual serialized collector publication owner without polling OS hardware.
use meowndesk_lib::telemetry::{
    clock::FakeClock,
    collector::*,
    model::*,
    providers::{fixtures::*, *},
    scopes::{GpuCandidate, GpuIdentityKind},
};
fn gpu(id: &str) -> GpuCandidate {
    GpuCandidate::new(
        id,
        id,
        GpuVendor::Nvidia,
        GpuKind::Discrete,
        true,
        GpuIdentityKind::NvmlUuid,
    )
}
fn host() -> HostIdentity {
    let j: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json")).unwrap();
    serde_json::from_value(j["bootstrap"]["view"]["sample"]["host"].clone()).unwrap()
}
fn poll(t: u64) -> ProviderPoll {
    let clock = FakeClock::new(t, 1000 + t as i64);
    let mut poll = FixtureProvider::new(FixtureScenario::Full).poll(&PollContext {
        clock: &clock,
        elapsed_ms: t,
        interval_ms: 1000,
    });
    poll.processes = vec![ProcessObservation {
        key: "1:100".into(),
        pid: 1,
        name: "Readable process".into(),
        resident_bytes: Reading::valid(ByteCount(12345), 1000 + t as i64, t, HOST_SCOPE),
        cpu_pct: Reading::valid(1.0, 1000 + t as i64, t, CPU_SCOPE),
        gpu_pct: Reading {
            scope_id: None,
            ..Reading::unsupported("none", "No attribution")
        },
        power_w: Reading {
            scope_id: None,
            ..Reading::unsupported("none", "No measured process watts")
        },
    }];
    poll
}
fn publish(service: &TelemetryService, t: u64, poll: ProviderPoll) {
    service
        .publish(
            &service.capture_poll(),
            t,
            1000 + t as i64,
            host(),
            vec![poll],
        )
        .unwrap();
    service.bootstrap().view.validate().unwrap();
}
#[test]
fn coherent_publication_degrades_and_recovers_without_erasing_working_fields() {
    let service = TelemetryService::new();
    service.reconcile_gpus(vec![gpu("GPU-fixture")]);
    publish(&service, 0, poll(0));
    let mut failed = poll(1000);
    failed.cpu.as_mut().unwrap().total_pct = Reading::permission_denied(CPU_SCOPE, "Access denied");
    publish(&service, 1000, failed);
    let view = service.bootstrap().view;
    assert!(
        view.notices
            .iter()
            .any(|n| n.code == "baseline_degraded" && n.message.contains("cpu.totalPct"))
    );
    assert_eq!(
        view.sample.as_ref().unwrap().memory.used_pct.status,
        ReadingStatus::Valid
    );
    publish(&service, 2000, poll(2000));
    let view = service.bootstrap().view;
    assert!(!view.notices.iter().any(|n| n.code == "baseline_degraded"));
    assert_eq!(
        view.endpoint_elapsed_ms,
        view.history.as_ref().unwrap().endpoint_elapsed_ms
    );
    assert_eq!(view.sample.as_ref().unwrap().sequence, 3);
}
#[test]
fn delayed_gpu_poll_cannot_restore_old_device_or_power_history() {
    let service = TelemetryService::new();
    service.reconcile_gpus(vec![gpu("GPU-fixture")]);
    let mut first = poll(0);
    first.power = None;
    publish(&service, 0, first);
    let stamp = service.capture_poll();
    let old = service.bootstrap().view;
    let old_cpu = old.sample.as_ref().unwrap().cpu.clone();
    service.reconcile_gpus(vec![gpu("GPU-next")]);
    let changed = service.bootstrap().view;
    changed.validate().unwrap();
    assert_eq!(
        changed.sample.as_ref().unwrap().gpu.utilization_pct.status,
        ReadingStatus::Unavailable
    );
    assert_eq!(changed.sample.as_ref().unwrap().cpu, old_cpu);
    assert!(
        changed.history.as_ref().unwrap().charts[2]
            .columns
            .iter()
            .all(|c| c.value.is_none())
    );
    assert!(
        changed.history.as_ref().unwrap().charts[5]
            .columns
            .iter()
            .all(|c| c.value.is_none())
    );
    let mut late = poll(1000);
    late.power = None;
    service
        .publish(&stamp, 1000, 2000, host(), vec![late])
        .unwrap();
    let next = service.bootstrap().view;
    next.validate().unwrap();
    assert!(
        next.sample
            .as_ref()
            .unwrap()
            .gpu
            .utilization_pct
            .value
            .is_none()
    );
    assert_eq!(
        next.sample
            .as_ref()
            .unwrap()
            .gpu
            .utilization_pct
            .scope_id
            .as_deref(),
        Some("gpu:GPU-next")
    );
    assert_eq!(
        next.sample.as_ref().unwrap().cpu.total_pct.status,
        ReadingStatus::Valid
    );
}
#[test]
fn empty_interfaces_are_truthful_and_close_releases_all_cached_session_readings() {
    let service = TelemetryService::new();
    service.reconcile_gpus(vec![gpu("GPU-fixture")]);
    publish(&service, 0, poll(0));
    let mut empty = poll(1000);
    empty.network_members = Some(meowndesk_lib::telemetry::bounds::network_member_metadata(
        &[],
    ));
    publish(&service, 1000, empty);
    assert!(
        service
            .bootstrap()
            .view
            .notices
            .iter()
            .any(|n| n.code == "network_empty")
    );
    assert!(
        !service
            .bootstrap()
            .view
            .notices
            .iter()
            .any(|n| n.code == "baseline_degraded")
    );
    let stamp = service.capture_poll();
    service.close();
    assert!(service.bootstrap().view.sample.is_none());
    assert!(service.bootstrap().view.history.is_none());
    assert!(service.bootstrap().gpus.is_empty());
    assert_eq!(service.counts(), (0, 0, 0));
    service
        .publish(&stamp, 2000, 3000, host(), vec![poll(2000)])
        .unwrap();
    assert!(service.bootstrap().view.sample.is_none());
}

#[test]
fn no_gpu_or_power_still_publishes_a_valid_whole_view_and_unavailable_charts() {
    let service = TelemetryService::new();
    let mut absent = poll(0);
    absent.gpu = None;
    absent.power = None;
    publish(&service, 0, absent);
    let view = service.bootstrap().view;
    assert_eq!(
        view.sample.as_ref().unwrap().gpu.utilization_pct.status,
        ReadingStatus::Unsupported
    );
    for index in [2, 5] {
        assert_eq!(
            view.history.as_ref().unwrap().charts[index].status,
            ReadingStatus::Unavailable
        );
        assert!(
            view.history.as_ref().unwrap().charts[index]
                .columns
                .iter()
                .all(|c| c.value.is_none())
        );
    }
    assert_eq!(
        view.sample.as_ref().unwrap().memory.used_pct.status,
        ReadingStatus::Valid
    );
}
