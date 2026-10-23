//! US3 memory/process domain acceptance written before the breakdown implementation.
use meowndesk_lib::telemetry::{
    memory, model::*, processes, providers::system::normalize_process_cpu,
};

fn valid<T>(value: T, scope: &str) -> Reading<T> {
    Reading::valid(value, 1_000, 1_000, scope)
}

fn unavailable<T>(scope: &str, reason: &str) -> Reading<T> {
    Reading::unavailable(scope, reason)
}

fn memory(total: u64, available: u64) -> MemoryMetrics {
    MemoryMetrics {
        total_bytes: valid(ByteCount(total), "host:system"),
        used_bytes: valid(ByteCount(total - available), "host:system"),
        available_bytes: valid(ByteCount(available), "host:system"),
        cached_bytes: valid(ByteCount(available / 2), "host:system"),
        used_pct: valid(
            100.0 * (total - available) as f64 / total as f64,
            "host:system",
        ),
    }
}

fn gpu(
    total: Option<u64>,
    used: Option<u64>,
    free: Option<u64>,
    reserved: Option<u64>,
) -> GpuMetrics {
    GpuMetrics {
        utilization_pct: valid(10.0, "gpu:A"),
        vram_total_bytes: total.map_or_else(
            || unavailable("gpu:A", "No dedicated VRAM"),
            |v| valid(ByteCount(v), "gpu:A"),
        ),
        vram_used_bytes: used.map_or_else(
            || unavailable("gpu:A", "No used VRAM"),
            |v| valid(ByteCount(v), "gpu:A"),
        ),
        vram_free_bytes: free.map_or_else(
            || unavailable("gpu:A", "No free VRAM"),
            |v| valid(ByteCount(v), "gpu:A"),
        ),
        vram_reserved_bytes: reserved.map_or_else(
            || Reading::unsupported("gpu:A", "No reserved VRAM"),
            |v| valid(ByteCount(v), "gpu:A"),
        ),
        temperature_c: unavailable("gpu:A", "none"),
        power_w: unavailable("gpu:A", "none"),
        fan_pct: unavailable("gpu:A", "none"),
        power_limit_pct: unavailable("gpu:A", "none"),
    }
}

fn process(pid: u32, key: &str, resident: u64, cpu: f64) -> ProcessObservation {
    ProcessObservation {
        key: key.into(),
        pid,
        name: format!("process-{pid}"),
        resident_bytes: valid(ByteCount(resident), "host:system"),
        cpu_pct: valid(cpu, "cpu:system"),
        gpu_pct: Reading {
            scope_id: None,
            ..Reading::unsupported("gpu:none", "No attribution")
        },
        power_w: Reading {
            scope_id: None,
            ..Reading::unsupported("power:none", "No process watts")
        },
    }
}

#[test]
fn ram_is_an_exact_in_use_available_partition_and_cache_is_not_a_slice() {
    let breakdown = memory::ram(&memory(10_000, 4_000)).expect("valid RAM partition");
    assert_eq!(breakdown.kind, MemoryView::Ram);
    assert_eq!(breakdown.total_bytes.0, 10_000);
    assert_eq!(breakdown.used_pct, 60.0);
    assert_eq!(breakdown.coverage, BreakdownCoverage::Complete);
    assert_eq!(breakdown.segments.len(), 2);
    assert_eq!(breakdown.segments[0].role, SegmentRole::Used);
    assert_eq!(breakdown.segments[0].bytes.0, 6_000);
    assert_eq!(breakdown.segments[1].role, SegmentRole::Available);
    assert_eq!(breakdown.segments[1].bytes.0, 4_000);
    assert!(!breakdown.segments.iter().any(|s| s.label.contains("Cache")));
}

#[test]
fn vram_uses_disjoint_categories_and_makes_partial_coverage_explicit() {
    let complete = memory::vram(&gpu(Some(100), Some(60), Some(30), Some(10))).unwrap();
    assert_eq!(complete.coverage, BreakdownCoverage::Complete);
    assert_eq!(complete.segments.len(), 3);
    assert_eq!(complete.used_pct, 60.0);

    let partial = memory::vram(&gpu(Some(100), Some(50), None, None)).unwrap();
    assert_eq!(partial.coverage, BreakdownCoverage::Partial);
    assert_eq!(partial.segments.len(), 2);
    assert_eq!(partial.segments[1].role, SegmentRole::Unclassified);
    assert_eq!(partial.segments[1].bytes.0, 50);
    assert!(partial.note.unwrap().contains("Unclassified"));

    let unavailable = memory::vram(&gpu(None, Some(10), None, None));
    assert!(unavailable.is_none());
    assert!(memory::vram(&gpu(Some(100), Some(80), Some(30), None)).is_none());
    assert!(memory::vram(&gpu(Some(100), Some(80), Some(10), Some(20))).is_none());
}

#[test]
fn used_percent_is_not_rounded_before_threshold_evaluation() {
    for (used, expected) in [(7_999, 79.99), (8_000, 80.0), (8_999, 89.99), (9_000, 90.0)] {
        let breakdown = memory::ram(&memory(10_000, 10_000 - used)).unwrap();
        assert!((breakdown.used_pct - expected).abs() < 1e-9);
    }
}

#[test]
fn process_cpu_is_normalized_once_and_summarization_does_not_divide_again() {
    let normalized = normalize_process_cpu(400.0, 8).expect("400% across eight logical CPUs");
    assert_eq!(normalized, 50.0);
    let summaries = processes::summarize(&[process(1, "1:1", 100, normalized)], None);
    assert_eq!(summaries.cpu_top.len(), 1);
    assert_eq!(summaries.cpu_top[0].cpu_pct.value, Some(50.0));
}

#[test]
fn top_lists_are_bounded_ordered_by_raw_value_then_key_and_exclude_invalid_entries() {
    let mut list = Vec::new();
    for index in 0..20u32 {
        list.push(process(
            index + 1,
            &format!("{}:1", index + 1),
            u64::from(index) * 1_000,
            f64::from(index) * 5.0,
        ));
    }
    let tie_a = process(30, "30:1", 19_000, 95.0);
    let tie_b = process(31, "31:1", 19_000, 95.0);
    list.push(tie_a);
    list.push(tie_b);
    let mut invalid = process(99, "99:1", 999_999, 100.0);
    invalid.resident_bytes = Reading::stale(
        Some(ByteCount(999_999)),
        Some(1_000),
        Some(1_000),
        "host:system",
        "old",
    );
    invalid.cpu_pct = Reading::unavailable("cpu:system", "No current reading");
    list.push(invalid);

    let summaries = processes::summarize(&list, None);
    assert_eq!(summaries.memory_top.len(), MAX_PROCESS_MEMORY_ENTRIES);
    assert_eq!(summaries.cpu_top.len(), MAX_PROCESS_RANK_ENTRIES);
    assert_eq!(summaries.gpu_top.len(), 0);
    assert_eq!(summaries.power_top.len(), 0);
    assert!(
        summaries
            .memory_top
            .windows(2)
            .all(|pair| pair[0].resident_bytes.value >= pair[1].resident_bytes.value)
    );
    assert!(
        summaries
            .cpu_top
            .windows(2)
            .all(|pair| pair[0].cpu_pct.value.unwrap_or_default()
                >= pair[1].cpu_pct.value.unwrap_or_default())
    );
    let displayed: u64 = summaries
        .memory_top
        .iter()
        .filter_map(|p| p.resident_bytes.value.map(|v| v.0))
        .sum();
    assert_eq!(summaries.displayed_memory_total_bytes.0, displayed);
    assert!(summaries.memory_top.iter().all(|p| p.key != "99:1"));
    assert!(summaries.cpu_top.iter().all(|p| p.key != "99:1"));
    assert_eq!(
        summaries.supported_ranking_metrics,
        vec![RankingMetric::Cpu]
    );
}

#[test]
fn app_aggregation_keeps_a_deterministic_representative_and_per_pid_gpu_identity() {
    let mut first = process(7, "7:100", 1_000, 10.0);
    let mut reused = process(7, "7:900", 2_000, 10.0);
    first.gpu_pct = valid(5.0, "gpu:A");
    reused.gpu_pct = valid(10.0, "gpu:A");
    let summaries = processes::summarize(&[reused, first], Some("gpu:A"));
    assert_eq!(summaries.memory_top.len(), 1);
    assert_eq!(summaries.memory_top[0].key, "7:100");
    assert_eq!(
        summaries.memory_top[0].resident_bytes.value,
        Some(ByteCount(3_000))
    );
    assert_eq!(summaries.cpu_top[0].cpu_pct.value, Some(20.0));
    assert_eq!(summaries.gpu_top[0].key, "7:900");
    assert_eq!(summaries.gpu_top[1].key, "7:100");
}

#[test]
fn empty_lists_keep_cpu_available_without_inventing_entries() {
    let summaries = processes::summarize(&[], None);
    assert!(summaries.memory_top.is_empty());
    assert!(summaries.cpu_top.is_empty());
    assert!(summaries.gpu_top.is_empty());
    assert!(summaries.power_top.is_empty());
    assert_eq!(summaries.displayed_memory_total_bytes.0, 0);
    assert_eq!(
        summaries.supported_ranking_metrics,
        vec![RankingMetric::Cpu]
    );
    assert!(
        summaries
            .coverage_notes
            .iter()
            .any(|n| n.contains("shared"))
    );
}

#[test]
fn gpu_rankings_require_the_selected_adapter_scope_and_never_use_unattributed_power() {
    let mut attributed = process(1, "1:1", 1_000, 5.0);
    attributed.gpu_pct = valid(42.0, "gpu:A");
    let mut other = process(2, "2:1", 1_000, 1.0);
    other.gpu_pct = valid(99.0, "gpu:B");
    let mut no_scope = process(3, "3:1", 1_000, 2.0);
    no_scope.gpu_pct = Reading::unavailable("gpu:A", "No attribution");
    let summaries = processes::summarize(&[attributed, other, no_scope], Some("gpu:A"));
    assert_eq!(summaries.gpu_top.len(), 1);
    assert_eq!(summaries.gpu_top[0].pid, 1);
    assert!(
        summaries
            .supported_ranking_metrics
            .contains(&RankingMetric::Gpu)
    );
    assert!(
        !summaries
            .supported_ranking_metrics
            .contains(&RankingMetric::Power)
    );
    assert!(summaries.power_top.is_empty());

    let without_scope = processes::summarize(&[process(1, "1:1", 1_000, 5.0)], None);
    assert!(without_scope.gpu_top.is_empty());
    assert!(
        !without_scope
            .supported_ranking_metrics
            .contains(&RankingMetric::Gpu)
    );
}

#[test]
fn windows_per_process_gpu_sums_engines_then_uses_the_busiest_engine() {
    use meowndesk_lib::telemetry::providers::windows::gpu::{
        EngineCounter, busiest_engine, per_process_utilization,
    };
    let counter = |instance: &str, physical: u32, engine: u32, pid: u32, pct: f64| EngineCounter {
        instance: instance.into(),
        luid: 7,
        physical,
        engine,
        pid,
        pct,
    };
    let mut foreign = counter("other_luid_pid_3", 0, 0, 3, 99.0);
    foreign.luid = 8;
    let values = vec![
        counter("luid_7_phys_0_eng_0_pid_1", 0, 0, 1, 30.0),
        counter("luid_7_phys_0_eng_1_pid_1", 0, 1, 1, 20.0),
        counter("luid_7_phys_0_eng_0_pid_2", 0, 0, 2, 60.0),
        foreign,
    ];
    let per_process = per_process_utilization(7, &values).unwrap();
    assert_eq!(per_process.get(&1), Some(&30.0));
    assert_eq!(per_process.get(&2), Some(&60.0));
    assert_eq!(per_process.get(&3), None);
    assert_eq!(busiest_engine(7, &values), Some(90.0));

    let conflicting = vec![counter("a", 0, 0, 1, 70.0), counter("b", 0, 0, 1, 50.0)];
    assert!(per_process_utilization(7, &conflicting).is_none());
}

fn named_process(pid: u32, name: &str, resident: u64, cpu: f64) -> ProcessObservation {
    ProcessObservation {
        name: name.into(),
        ..process(pid, &format!("{pid}:1"), resident, cpu)
    }
}

#[test]
fn app_totals_are_computed_before_either_top_list_is_truncated() {
    let mut list: Vec<_> = (1..=10)
        .map(|pid| named_process(pid, &format!("app-{pid}"), 1_000, 5.0))
        .collect();
    for pid in 11..=30 {
        list.push(named_process(
            pid,
            if pid % 2 == 0 { "CHROME.EXE" } else { "chrome" },
            100,
            0.5,
        ));
    }
    let summaries = processes::summarize(&list, None);
    assert_eq!(summaries.memory_top.len(), 8);
    assert_eq!(summaries.cpu_top.len(), 5);
    assert_eq!(summaries.memory_top[0].name, "Chrome");
    assert_eq!(
        summaries.memory_top[0].resident_bytes.value,
        Some(ByteCount(2_000))
    );
    assert_eq!(summaries.cpu_top[0].name, "Chrome");
    assert_eq!(summaries.cpu_top[0].cpu_pct.value, Some(10.0));
    assert_eq!(summaries.displayed_memory_total_bytes.0, 9_000);
    list.reverse();
    assert_eq!(summaries, processes::summarize(&list, None));
}

#[test]
fn normalized_executables_have_friendly_names_and_keep_unrelated_apps_separate() {
    for (names, expected) in [
        (vec!["vmmemWSL", "VMMEM"], "WSL2"),
        (vec!["Code.exe", "code"], "VS Code"),
        (vec!["msedge.exe", "MSEDGE.EXE"], "Edge"),
        (vec!["explorer.exe", "explorer"], "Explorer"),
        (vec!["opencode.exe", "opencode"], "OpenCode"),
        (vec!["docker.exe", "docker"], "Docker"),
        (vec!["OLLAMA.EXE", "ollama"], "Ollama"),
        (vec!["sharex.exe", "ShareX"], "ShareX"),
        (vec!["CustomApp.EXE"], "CustomApp"),
        (vec!["cödé — 開発環境.exe"], "cödé — 開発環境"),
    ] {
        let list: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(index, name)| named_process(index as u32 + 1, name, 500, 0.25))
            .collect();
        let summaries = processes::summarize(&list, None);
        assert_eq!(summaries.memory_top.len(), 1);
        assert_eq!(summaries.memory_top[0].name, expected);
        assert_eq!(
            summaries.memory_top[0].resident_bytes.value,
            Some(ByteCount(500 * names.len() as u64))
        );
    }
    let separate = processes::summarize(
        &[
            named_process(1, "code", 1, 1.0),
            named_process(2, "opencode", 1, 1.0),
        ],
        None,
    );
    assert_eq!(separate.memory_top.len(), 2);
}

#[test]
fn app_metrics_sum_only_current_readings_independently_and_remove_exited_members() {
    let mut first = named_process(1, "chrome.exe", 100, 0.1);
    let mut second = named_process(2, "chrome", 200, 0.2);
    first.cpu_pct = unavailable("cpu:system", "warming up");
    second.resident_bytes = Reading::stale(
        Some(ByteCount(200)),
        Some(1_000),
        Some(1_000),
        "host:system",
        "old",
    );
    let third = named_process(3, "Chrome.EXE", 300, 0.3);
    let summaries = processes::summarize(&[first.clone(), second, third], None);
    assert_eq!(
        summaries.memory_top[0].resident_bytes.value,
        Some(ByteCount(400))
    );
    assert_eq!(summaries.cpu_top[0].cpu_pct.value, Some(0.5));
    let after_exit = processes::summarize(&[first], None);
    assert_eq!(
        after_exit.memory_top[0].resident_bytes.value,
        Some(ByteCount(100))
    );
    assert!(after_exit.cpu_top.is_empty());
}

#[test]
fn app_memory_sums_remain_exact_above_js_precision_and_reject_overflow() {
    let summaries = processes::summarize(
        &[
            named_process(1, "code", 9_007_199_254_740_993, 0.1),
            named_process(2, "Code.exe", 2, 0.2),
        ],
        None,
    );
    assert_eq!(
        summaries.memory_top[0].resident_bytes.value,
        Some(ByteCount(9_007_199_254_740_995))
    );
    assert_eq!(
        summaries.displayed_memory_total_bytes.0,
        9_007_199_254_740_995
    );
    let overflow = processes::summarize(
        &[
            named_process(1, "code", u64::MAX, 0.1),
            named_process(2, "code", 1, 0.2),
            named_process(3, "code", 1, 0.3),
        ],
        None,
    );
    assert!(overflow.memory_top.is_empty());
    assert_eq!(overflow.cpu_top.len(), 1);
    assert_eq!(
        overflow.cpu_top[0].resident_bytes.status,
        ReadingStatus::Unavailable
    );
}

#[test]
fn incompatible_app_scopes_and_cpu_overflow_do_not_poison_other_metrics() {
    let first = named_process(1, "docker", 100, 60.0);
    let mut second = named_process(2, "DOCKER.EXE", 200, 50.0);
    let summaries = processes::summarize(&[first.clone(), second.clone()], None);
    assert!(summaries.cpu_top.is_empty());
    assert_eq!(
        summaries.memory_top[0].resident_bytes.value,
        Some(ByteCount(300))
    );
    assert_eq!(
        summaries.memory_top[0].cpu_pct.status,
        ReadingStatus::Unavailable
    );

    second.resident_bytes.scope_id = Some("host:other".into());
    second.cpu_pct.value = Some(10.0);
    let summaries = processes::summarize(&[first, second], None);
    assert!(summaries.memory_top.is_empty());
    assert_eq!(summaries.cpu_top[0].cpu_pct.value, Some(70.0));
    assert_eq!(
        summaries.cpu_top[0].resident_bytes.status,
        ReadingStatus::Unavailable
    );
}
