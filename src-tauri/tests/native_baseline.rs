//! US1 provider contract; written before the providers and FR-023 evaluator.
use meowndesk_lib::telemetry::{
    baseline::{BaselineState, evaluate},
    model::*,
    normalize::*,
    providers::{network::*, system::normalize_process_cpu, windows::gpu::*},
};
fn sample() -> CurrentSample {
    serde_json::from_str::<serde_json::Value>(include_str!("../../tests/fixtures/contract-v1.json"))
        .unwrap()["bootstrap"]["view"]["sample"]
        .clone()
        .try_into_sample()
}
trait SampleJson {
    fn try_into_sample(self) -> CurrentSample;
}
impl SampleJson for serde_json::Value {
    fn try_into_sample(self) -> CurrentSample {
        serde_json::from_value(self).unwrap()
    }
}
fn process() -> ProcessObservation {
    serde_json::from_str::<serde_json::Value>(include_str!("../../tests/fixtures/contract-v1.json"))
        .unwrap()["bootstrap"]["view"]["processSummaries"]["cpuTop"][0]
        .clone()
        .try_into_process()
}
trait ProcessJson {
    fn try_into_process(self) -> ProcessObservation;
}
impl ProcessJson for serde_json::Value {
    fn try_into_process(self) -> ProcessObservation {
        serde_json::from_value(self).unwrap()
    }
}
#[test]
fn each_required_failure_degrades_and_recovery_clears_after_warmup() {
    let good = sample();
    let proc = process();
    assert_eq!(
        evaluate(&good, std::slice::from_ref(&proc), false).state,
        BaselineState::Initializing
    );
    assert!(
        evaluate(&good, std::slice::from_ref(&proc), true)
            .notice
            .is_none()
    );
    for metric in [
        "cpu.totalPct",
        "memory.usedBytes",
        "memory.totalBytes",
        "disk.freeBytes",
        "disk.totalBytes",
        "network.receiveBytesPerSecond",
        "network.sendBytesPerSecond",
        "process.cpuPct",
        "process.residentBytes",
    ] {
        let mut json = serde_json::to_value(&good).unwrap();
        let mut process = serde_json::to_value(&proc).unwrap();
        let (group, field) = metric.split_once('.').unwrap();
        let target = if group == "process" {
            &mut process[field]
        } else {
            &mut json[group][field]
        };
        *target = serde_json::json!({"status":"permission_denied", "value":null,"sampledAtMs":null,"elapsedMs":null,"scopeId":target["scopeId"],"reason":"Access denied"});
        let failed: CurrentSample = serde_json::from_value(json).unwrap();
        let process: ProcessObservation = serde_json::from_value(process).unwrap();
        let result = evaluate(&failed, &[process], true);
        assert_eq!(result.state, BaselineState::Degraded, "{metric}");
        let notice = result.notice.unwrap();
        assert_eq!(notice.code, "baseline_degraded");
        assert!(notice.message.contains(metric), "{notice:?}");
        assert_eq!(failed.cpu.temperature_c, good.cpu.temperature_c);
        assert!(
            evaluate(&good, std::slice::from_ref(&proc), true)
                .notice
                .is_none()
        );
    }
    assert_eq!(evaluate(&good, &[], true).state, BaselineState::Degraded);
}
#[test]
fn optional_loss_and_empty_network_do_not_degrade_and_guest_is_not_native_acceptance() {
    let mut good = sample();
    good.cpu.temperature_c = Reading::unsupported("cpu:fixture-package", "No sensor");
    good.gpu.power_w = Reading::unavailable("gpu:GPU-fixture", "Driver unavailable");
    good.network.receive_bytes_per_second.value = Some(0.0);
    good.network.send_bytes_per_second.value = Some(0.0);
    assert_eq!(
        evaluate(&good, &[process()], true).state,
        BaselineState::Ready
    );
    good.host.environment = Environment::Wsl2;
    let result = evaluate(&good, &[process()], true);
    assert!(!result.native_accepted);
    assert!(result.notice.is_none());
}
#[test]
fn normalization_rejects_invalid_domains_and_preserves_real_zero() {
    assert_eq!(normalize_process_cpu(800.0, 8), Some(100.0));
    for (pct, cores) in [(101.0, 1), (-1.0, 2), (f64::NAN, 8), (1.0, 0)] {
        assert_eq!(normalize_process_cpu(pct, cores), None);
    }
    assert_eq!(ram_accounting(1000, 400), Some((600, 60.0)));
    assert_eq!(ram_accounting(0, 0), None);
    assert_eq!(ram_accounting(10, 11), None);
    assert_eq!(power_limit_pct(200.0, 250.0), Some(80.0));
    assert_eq!(power_limit_pct(250.0, 200.0), None);
    assert_eq!(
        combined_rate(
            &Reading::valid(0.0, 1, 1, "n"),
            &Reading::valid(0.0, 1, 1, "n")
        )
        .value,
        Some(0.0)
    );
    assert!(
        combined_rate(
            &Reading::valid(1.0, 1, 1, "n"),
            &Reading::unavailable("n", "Reset")
        )
        .value
        .is_none()
    );
}
#[test]
fn network_members_reset_warm_up_and_metadata_does_not_limit_collection() {
    let mut aggregate = NetworkTracker::default();
    let members: Vec<_> = (0..40)
        .map(|n| InterfaceCounters {
            id: format!("if:{n}"),
            up: true,
            loopback: false,
            received: 1000,
            sent: 2000,
        })
        .collect();
    let first = aggregate.observe(Ok(members.clone()), 0, 1000, 1);
    assert_eq!(
        first.metrics.receive_bytes_per_second.status,
        ReadingStatus::Unavailable
    );
    let next = members
        .iter()
        .cloned()
        .map(|mut m| {
            m.received += 100;
            m.sent += 200;
            m
        })
        .collect();
    let second = aggregate.observe(Ok(next), 1000, 1000, 2);
    assert_eq!(second.metrics.receive_bytes_per_second.value, Some(4000.0));
    assert_eq!(second.members.omitted_member_count, 8);
    assert_eq!(
        second.metrics.received_session_bytes.value,
        Some(ByteCount(4000))
    );
    let reset = aggregate.observe(Ok(members), 2000, 1000, 3);
    assert!(reset.metrics.receive_bytes_per_second.value.is_none());
    let empty = aggregate.observe(Ok(vec![]), 3000, 1000, 4);
    assert_eq!(empty.metrics.receive_bytes_per_second.value, Some(0.0));
    assert_eq!(
        empty.metrics.received_session_bytes.value,
        Some(ByteCount(4000))
    );
    assert!(
        aggregate
            .observe(
                Err(meowndesk_lib::telemetry::providers::ProviderError::PermissionDenied),
                4000,
                1000,
                5
            )
            .metrics
            .receive_bytes_per_second
            .value
            .is_none()
    );
}
#[test]
fn two_pids_on_one_physical_engine_sum_before_busiest_engine_selection() {
    let a = EngineCounter {
        instance: "pid_1_a".into(),
        luid: 7,
        physical: 0,
        engine: 0,
        pid: 1,
        pct: 50.0,
    };
    let b = EngineCounter {
        instance: "pid_2_a".into(),
        luid: 7,
        physical: 0,
        engine: 0,
        pid: 2,
        pct: 50.0,
    };
    assert_eq!(
        busiest_engine(7, &[a.clone(), a.clone(), b.clone()]),
        Some(100.0)
    );
    let mut invalid = b.clone();
    invalid.pct = 51.0;
    assert_eq!(busiest_engine(7, &[a, invalid]), None);
    assert_eq!(busiest_engine(8, &[b]), None);
}
#[cfg(target_os = "linux")]
#[test]
fn exact_volume_rejects_parent_disk_and_cpu_guest_time_is_not_double_counted() {
    use meowndesk_lib::telemetry::providers::linux::system::*;
    assert_eq!(disk_counters("8 0 sda 1 0 100 0 1 0 200 0", (8, 1)), None);
    assert_eq!(
        disk_counters("8 1 sda1 1 0 100 0 1 0 200 0", (8, 1)),
        Some((51200, 102400))
    );
    let old = parse_cpu("cpu 100 0 50 850 0 0 0 0 20 0").unwrap();
    let new = parse_cpu("cpu 150 0 70 880 0 0 0 0 30 0").unwrap();
    assert_eq!(cpu_split(old, new), Some((50.0, 20.0)));
}
#[test]
fn stale_retains_last_observation_and_explicit_denied_never_turns_stale() {
    let old = Reading::valid(42.0, 10, 1000, "cpu");
    let missing = Reading::unavailable("cpu", "Sensor lost");
    assert_eq!(
        retain_last(&missing, Some(&old), 3000, 1000).status,
        ReadingStatus::Unavailable
    );
    let stale = retain_last(&missing, Some(&old), 3001, 1000);
    assert_eq!(stale.status, ReadingStatus::Stale);
    assert_eq!(stale.value, Some(42.0));
    assert_eq!(stale.sampled_at_ms, Some(10));
    assert_eq!(
        retain_last(
            &Reading::permission_denied("cpu", "Denied"),
            Some(&old),
            9000,
            1000
        )
        .status,
        ReadingStatus::PermissionDenied
    );
    assert!(
        retain_last(
            &Reading::unavailable("other", "New scope"),
            Some(&old),
            9000,
            1000
        )
        .value
        .is_none()
    );
}
#[test]
fn merge_rejects_nonfinite_and_capacity_errors_and_keeps_battery_when_rapl_arrives() {
    use meowndesk_lib::telemetry::providers::ProviderPoll;
    let mut good = sample();
    let mut poll = ProviderPoll {
        cpu: Some(good.cpu.clone()),
        memory: Some(good.memory.clone()),
        disk: Some(good.disk.clone()),
        network: Some(good.network.clone()),
        gpu: Some(good.gpu.clone()),
        power: Some(good.power.clone()),
        ..Default::default()
    };
    poll.cpu.as_mut().unwrap().total_pct.value = Some(f64::NAN);
    poll.memory.as_mut().unwrap().total_bytes.value = Some(ByteCount(0));
    poll.disk.as_mut().unwrap().free_bytes.value = Some(ByteCount(u64::MAX));
    let mut rapl = empty_power("cpu:fixture-package");
    rapl.cpu_package_w = Reading::valid(
        35.0,
        good.captured_at_ms,
        good.elapsed_ms,
        "cpu:fixture-package",
    );
    let result = merge(
        good.clone(),
        &[
            poll,
            ProviderPoll {
                power: Some(rapl),
                ..Default::default()
            },
        ],
        None,
    );
    assert_eq!(result.cpu.total_pct.status, ReadingStatus::Unavailable);
    assert_eq!(result.memory.total_bytes.status, ReadingStatus::Unavailable);
    assert_eq!(result.disk.free_bytes.status, ReadingStatus::Unavailable);
    assert_eq!(result.power.primary_kind, PowerPrimaryKind::BatteryPct);
    assert_eq!(result.power.cpu_package_w.value, Some(35.0));
    // A valid GPU board query never substitutes for an unavailable AC battery discharge field.
    let mut power = good.power.clone();
    power.battery_state.value = Some(BatteryState::OnAc);
    choose_power(&mut power, &good.gpu);
    assert_eq!(power.watts.status, ReadingStatus::Unavailable);
    good.gpu.power_w = Reading::unsupported("gpu:GPU-fixture", "No board power");
    let mut desktop = empty_power("cpu:fixture-package");
    desktop.cpu_package_w = Reading::valid(35.0, 1, 1, "cpu:fixture-package");
    choose_power(&mut desktop, &good.gpu);
    assert_eq!(desktop.primary_kind, PowerPrimaryKind::CpuPackageWatts);
    assert_eq!(desktop.watts.value, Some(35.0));
}
#[test]
fn normalization_cache_survives_multiple_missing_polls_and_epoch_change_clears_it() {
    use meowndesk_lib::telemetry::providers::ProviderPoll;
    let good = sample();
    let mut n = Normalizer::default();
    n.normalize(
        good.clone(),
        &[ProviderPoll {
            cpu: Some(good.cpu.clone()),
            ..Default::default()
        }],
    );
    let mut frame = good.clone();
    frame.elapsed_ms += 1000;
    n.normalize(frame.clone(), &[]);
    frame.elapsed_ms += 1001;
    let output = n.normalize(frame.clone(), &[]);
    assert_eq!(output.cpu.total_pct.status, ReadingStatus::Stale);
    assert_eq!(output.cpu.total_pct.value, good.cpu.total_pct.value);
    frame.scopes.iter_mut().for_each(|s| s.epoch += 1);
    let output = n.normalize(frame, &[]);
    assert_eq!(output.cpu.total_pct.status, ReadingStatus::Unavailable);
    assert!(output.cpu.total_pct.value.is_none());
}
#[cfg(target_os = "linux")]
#[test]
fn rapl_wrap_uses_microjoules_and_does_not_bridge_suspend() {
    use meowndesk_lib::telemetry::providers::linux::power::RaplTracker;
    let mut rapl = RaplTracker::default();
    assert_eq!(
        rapl.observe("package-0", 9_000_000, 10_000_000, 0, 1000),
        None
    );
    assert_eq!(
        rapl.observe("package-0", 1_000_000, 10_000_000, 1000, 1000),
        Some(2.0)
    );
    assert_eq!(
        rapl.observe("package-0", 2_000_000, 10_000_000, 4001, 1000),
        None
    );
    assert_eq!(
        rapl.observe("package-0", 3_000_000, 10_000_000, 5001, 1000),
        Some(1.0)
    );
}
#[test]
fn windows_kernel_time_subtracts_idle_and_rejects_reset() {
    use meowndesk_lib::telemetry::providers::windows::system::{CpuTimes, cpu_split};
    let old = CpuTimes {
        idle: 850,
        kernel: 900,
        user: 100,
    };
    let new = CpuTimes {
        idle: 880,
        kernel: 950,
        user: 150,
    };
    assert_eq!(cpu_split(old, new), Some((50.0, 20.0)));
    assert_eq!(cpu_split(new, old), None);
}
