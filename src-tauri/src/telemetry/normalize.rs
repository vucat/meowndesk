//! Provider normalization. Historical publication/epoch race guards belong to T036.
use crate::telemetry::{bounds, model::*, providers::ProviderPoll};

pub fn ram_accounting(total: u64, available: u64) -> Option<(u64, f64)> {
    if total == 0 || available > total {
        return None;
    }
    let used = total - available;
    Some((used, 100.0 * used as f64 / total as f64))
}
pub fn power_limit_pct(configured: f64, maximum: f64) -> Option<f64> {
    (configured.is_finite()
        && maximum.is_finite()
        && configured > 0.0
        && maximum > 0.0
        && configured <= maximum)
        .then(|| 100.0 * configured / maximum)
}
pub fn combined_rate(a: &Reading<f64>, b: &Reading<f64>) -> Reading<f64> {
    if a.status == ReadingStatus::Valid
        && b.status == ReadingStatus::Valid
        && a.scope_id.is_some()
        && a.scope_id == b.scope_id
        && let (Some(av), Some(bv), Some(at), Some(bt), Some(ae), Some(be)) = (
            a.value,
            b.value,
            a.sampled_at_ms,
            b.sampled_at_ms,
            a.elapsed_ms,
            b.elapsed_ms,
        )
        && valid_nonnegative(&av)
        && valid_nonnegative(&bv)
        && valid_nonnegative(&(av + bv))
    {
        return Reading::valid(
            av + bv,
            at.min(bt),
            ae.min(be),
            a.scope_id.as_deref().unwrap_or("unavailable"),
        );
    }
    Reading {
        scope_id: a.scope_id.clone(),
        ..Reading::unavailable(
            "unavailable",
            "Both component rates must be valid at the same scope",
        )
    }
}
/// Retain only same-scope last-valid observations; never age explicit unsupported/denied states.
pub fn retain_last<T: Clone>(
    current: &Reading<T>,
    previous: Option<&Reading<T>>,
    now: u64,
    interval: u64,
) -> Reading<T> {
    if matches!(
        current.status,
        ReadingStatus::Unsupported | ReadingStatus::PermissionDenied
    ) {
        return current.clone();
    }
    let last = if current.status == ReadingStatus::Valid {
        Some(current)
    } else {
        previous.filter(|p| {
            p.scope_id == current.scope_id
                && p.value.is_some()
                && matches!(p.status, ReadingStatus::Valid | ReadingStatus::Stale)
        })
    };
    if let Some(last) = last
        && last
            .elapsed_ms
            .is_some_and(|t| now.saturating_sub(t) > interval.saturating_mul(2))
    {
        let mut out = last.clone();
        out.status = ReadingStatus::Stale;
        out.reason = Some("No fresh observation within two collection intervals".into());
        return out;
    }
    current.clone()
}
fn sanitize<T: Clone>(reading: &Reading<T>, domain: impl Fn(&T) -> bool) -> Reading<T> {
    let mut out = reading.clone();
    out.scope_id = out
        .scope_id
        .map(|s| bounds::bounded_label(&s, bounds::LABEL_MAX_BYTES));
    out.reason = out.reason.map(|s| bounds::bounded_message(&s));
    if matches!(
        out.status,
        ReadingStatus::Unsupported | ReadingStatus::PermissionDenied | ReadingStatus::Unavailable
    ) {
        out.value = None;
        out.sampled_at_ms = None;
        out.elapsed_ms = None;
        if out.reason.is_none() {
            out.reason = Some("No valid observation is available".into());
        }
    } else if out.value.as_ref().is_some_and(|v| !domain(v))
        || out.sampled_at_ms.is_some_and(|t| t < 0)
        || (out.value.is_some() && (out.sampled_at_ms.is_none() || out.elapsed_ms.is_none()))
        || (out.status == ReadingStatus::Valid && (out.value.is_none() || out.scope_id.is_none()))
    {
        out = Reading::unavailable(
            out.scope_id.as_deref().unwrap_or("unavailable"),
            "The provider returned an invalid observation",
        );
    }
    if out.status == ReadingStatus::Valid {
        out.reason = None;
    }
    if out.status == ReadingStatus::Stale {
        if out.value.is_none() {
            out.sampled_at_ms = None;
            out.elapsed_ms = None;
        }
        if out.reason.is_none() {
            out.reason = Some("No fresh observation is available".into());
        }
    }
    out
}
pub fn empty_cpu(scope: &str) -> CpuMetrics {
    CpuMetrics {
        total_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        user_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        system_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        temperature_c: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}
pub fn empty_memory(scope: &str) -> MemoryMetrics {
    MemoryMetrics {
        total_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        used_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        available_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        cached_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        used_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}
pub fn empty_gpu(scope: &str) -> GpuMetrics {
    GpuMetrics {
        utilization_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        vram_total_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        vram_used_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        vram_free_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        vram_reserved_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        temperature_c: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        power_w: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        fan_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        power_limit_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}
pub fn empty_disk(scope: &str) -> DiskMetrics {
    DiskMetrics {
        total_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        free_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        read_bytes_per_second: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        write_bytes_per_second: Reading::unavailable(
            scope,
            "Awaiting a valid provider observation",
        ),
        written_session_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}
pub fn empty_network(scope: &str) -> NetworkMetrics {
    NetworkMetrics {
        receive_bytes_per_second: Reading::unavailable(
            scope,
            "Awaiting a valid provider observation",
        ),
        send_bytes_per_second: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        received_session_bytes: Reading::unavailable(
            scope,
            "Awaiting a valid provider observation",
        ),
        sent_session_bytes: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}
pub fn empty_power(scope: &str) -> PowerMetrics {
    PowerMetrics {
        primary_kind: PowerPrimaryKind::Unavailable,
        primary_value: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        watts: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        cpu_package_w: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        gpu_board_w: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        gpu_temperature_c: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        fan_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        power_limit_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        battery_charge_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        battery_health_pct: Reading::unavailable(scope, "Awaiting a valid provider observation"),
        battery_remaining_seconds: Reading::unavailable(
            scope,
            "Awaiting a valid provider observation",
        ),
        battery_state: Reading::unavailable(scope, "Awaiting a valid provider observation"),
    }
}

/// Merge independent native fields into a complete current sample. The caller supplies the
/// captured scope epochs and rejects obsolete GPU polls under its serialized owner (T036).
/// Absent groups are explicit loss, never a repeat of a previously successful poll.
pub fn merge(
    mut frame: CurrentSample,
    polls: &[ProviderPoll],
    previous: Option<&CurrentSample>,
) -> CurrentSample {
    let expected_gpu = frame.gpu.utilization_pct.scope_id.clone();
    frame.cpu = empty_cpu(
        frame
            .cpu
            .total_pct
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    frame.memory = empty_memory(
        frame
            .memory
            .total_bytes
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    frame.disk = empty_disk(
        frame
            .disk
            .total_bytes
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    frame.network = empty_network(
        frame
            .network
            .receive_bytes_per_second
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    frame.power = empty_power(
        frame
            .power
            .watts
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    frame.gpu = empty_gpu(
        frame
            .gpu
            .utilization_pct
            .scope_id
            .as_deref()
            .unwrap_or("unavailable"),
    );
    for poll in polls {
        if let Some(v) = &poll.cpu {
            prefer(&mut frame.cpu.total_pct, &v.total_pct);
            prefer(&mut frame.cpu.user_pct, &v.user_pct);
            prefer(&mut frame.cpu.system_pct, &v.system_pct);
            prefer(&mut frame.cpu.temperature_c, &v.temperature_c);
        }
        if let Some(v) = &poll.memory {
            prefer(&mut frame.memory.total_bytes, &v.total_bytes);
            prefer(&mut frame.memory.used_bytes, &v.used_bytes);
            prefer(&mut frame.memory.available_bytes, &v.available_bytes);
            prefer(&mut frame.memory.cached_bytes, &v.cached_bytes);
            prefer(&mut frame.memory.used_pct, &v.used_pct);
        }
        if let Some(v) = &poll.disk {
            prefer(&mut frame.disk.total_bytes, &v.total_bytes);
            prefer(&mut frame.disk.free_bytes, &v.free_bytes);
            prefer(
                &mut frame.disk.read_bytes_per_second,
                &v.read_bytes_per_second,
            );
            prefer(
                &mut frame.disk.write_bytes_per_second,
                &v.write_bytes_per_second,
            );
            prefer(
                &mut frame.disk.written_session_bytes,
                &v.written_session_bytes,
            );
        }
        if let Some(v) = &poll.network {
            prefer(
                &mut frame.network.receive_bytes_per_second,
                &v.receive_bytes_per_second,
            );
            prefer(
                &mut frame.network.send_bytes_per_second,
                &v.send_bytes_per_second,
            );
            prefer(
                &mut frame.network.received_session_bytes,
                &v.received_session_bytes,
            );
            prefer(&mut frame.network.sent_session_bytes, &v.sent_session_bytes);
        }
        if let Some(v) = &poll.power {
            prefer(&mut frame.power.primary_value, &v.primary_value);
            prefer(&mut frame.power.watts, &v.watts);
            prefer(&mut frame.power.cpu_package_w, &v.cpu_package_w);
            prefer(&mut frame.power.gpu_board_w, &v.gpu_board_w);
            prefer(&mut frame.power.gpu_temperature_c, &v.gpu_temperature_c);
            prefer(&mut frame.power.fan_pct, &v.fan_pct);
            prefer(&mut frame.power.power_limit_pct, &v.power_limit_pct);
            prefer(&mut frame.power.battery_charge_pct, &v.battery_charge_pct);
            prefer(&mut frame.power.battery_health_pct, &v.battery_health_pct);
            prefer(
                &mut frame.power.battery_remaining_seconds,
                &v.battery_remaining_seconds,
            );
            prefer(&mut frame.power.battery_state, &v.battery_state);
        }
        if let Some(gpu) = &poll.gpu {
            // Mixing fields from different adapters would put old-device values under a new ID.
            if gpu.utilization_pct.scope_id == expected_gpu
                && frame.gpu.utilization_pct.status != ReadingStatus::Valid
            {
                frame.gpu = gpu.clone();
            }
        }
    }
    // RAM used is always derived from usable capacity and OS available memory. Keep unrelated
    // cache and capacity observations when one input fails; derived fields become explicit gaps.
    let memory_scope = frame
        .memory
        .total_bytes
        .scope_id
        .as_deref()
        .unwrap_or("unavailable")
        .to_owned();
    match valid_bytes(&frame.memory.total_bytes)
        .zip(valid_bytes(&frame.memory.available_bytes))
        .and_then(|(t, a)| ram_accounting(t, a))
    {
        Some((used, pct))
            if frame.memory.total_bytes.scope_id == frame.memory.available_bytes.scope_id =>
        {
            frame.memory.used_bytes = Reading {
                value: Some(ByteCount(used)),
                ..frame.memory.total_bytes.clone()
            };
            frame.memory.used_pct = Reading::valid(
                pct,
                frame
                    .memory
                    .total_bytes
                    .sampled_at_ms
                    .unwrap_or(frame.captured_at_ms),
                frame
                    .memory
                    .total_bytes
                    .elapsed_ms
                    .unwrap_or(frame.elapsed_ms),
                &memory_scope,
            );
        }
        _ => {
            frame.memory.used_bytes = Reading::unavailable(
                &memory_scope,
                "Usable and available RAM are required for accounting",
            );
            frame.memory.used_pct = Reading::unavailable(
                &memory_scope,
                "Usable and available RAM are required for accounting",
            );
            if valid_bytes(&frame.memory.total_bytes)
                .zip(valid_bytes(&frame.memory.available_bytes))
                .is_some_and(|(t, a)| a > t)
            {
                frame.memory.available_bytes =
                    Reading::unavailable(&memory_scope, "Available RAM exceeds usable capacity");
            }
        }
    }
    if valid_bytes(&frame.disk.total_bytes)
        .zip(valid_bytes(&frame.disk.free_bytes))
        .is_some_and(|(total, free)| free > total)
    {
        frame.disk.free_bytes = Reading::unavailable(
            frame
                .disk
                .free_bytes
                .scope_id
                .as_deref()
                .unwrap_or("unavailable"),
            "Free bytes exceed volume capacity",
        );
    }
    if let Some(total) = valid_bytes(&frame.gpu.vram_total_bytes) {
        let sum: u128 = [
            &frame.gpu.vram_used_bytes,
            &frame.gpu.vram_free_bytes,
            &frame.gpu.vram_reserved_bytes,
        ]
        .iter()
        .filter_map(|r| valid_bytes(r))
        .map(u128::from)
        .sum();
        if sum > u128::from(total) || total == 0 {
            let scope = frame
                .gpu
                .vram_total_bytes
                .scope_id
                .as_deref()
                .unwrap_or("unavailable")
                .to_owned();
            for r in [
                &mut frame.gpu.vram_used_bytes,
                &mut frame.gpu.vram_free_bytes,
                &mut frame.gpu.vram_reserved_bytes,
            ] {
                *r = Reading::unavailable(&scope, "Invalid dedicated VRAM accounting");
            }
        }
    }
    macro_rules! normalize_fields {
        ($group:ident, $( $field:ident => $domain:expr ),+ $(,)?) => {$(
            frame.$group.$field = retain_last(
                &sanitize(&frame.$group.$field, $domain),
                previous.filter(|old| same_epoch(old, &frame, frame.$group.$field.scope_id.as_deref())).map(|old| &old.$group.$field),
                frame.elapsed_ms, frame.interval_ms,
            );
        )+};
    }
    normalize_fields!(cpu, total_pct => valid_pct, user_pct => valid_pct, system_pct => valid_pct, temperature_c => valid_temperature);
    normalize_fields!(memory, total_bytes => |v| v.0 > 0, used_bytes => |_| true, available_bytes => |_| true, cached_bytes => |_| true, used_pct => valid_pct);
    normalize_fields!(gpu, utilization_pct => valid_pct, vram_total_bytes => |v| v.0 > 0, vram_used_bytes => |_| true, vram_free_bytes => |_| true, vram_reserved_bytes => |_| true, temperature_c => valid_temperature, power_w => valid_nonnegative, fan_pct => valid_pct, power_limit_pct => valid_pct);
    normalize_fields!(disk, total_bytes => |v| v.0 > 0, free_bytes => |_| true, read_bytes_per_second => valid_nonnegative, write_bytes_per_second => valid_nonnegative, written_session_bytes => |_| true);
    normalize_fields!(network, receive_bytes_per_second => valid_nonnegative, send_bytes_per_second => valid_nonnegative, received_session_bytes => |_| true, sent_session_bytes => |_| true);
    normalize_fields!(power, primary_value => valid_nonnegative, watts => valid_nonnegative, cpu_package_w => valid_nonnegative, gpu_board_w => valid_nonnegative, gpu_temperature_c => valid_temperature, fan_pct => valid_pct, power_limit_pct => valid_pct, battery_charge_pct => valid_pct, battery_health_pct => valid_pct, battery_remaining_seconds => |v| *v <= MAX_JSON_SAFE_INT, battery_state => |_| true);
    choose_power(&mut frame.power, &frame.gpu);
    frame
}
fn valid_bytes(r: &Reading<ByteCount>) -> Option<u64> {
    (r.status == ReadingStatus::Valid)
        .then_some(r.value)
        .flatten()
        .map(|v| v.0)
}
fn same_epoch(old: &CurrentSample, new: &CurrentSample, id: Option<&str>) -> bool {
    id.is_some_and(|id| {
        old.scopes
            .iter()
            .find(|s| s.id == id)
            .zip(new.scopes.iter().find(|s| s.id == id))
            .is_some_and(|(a, b)| a.epoch == b.epoch)
    })
}
pub fn choose_power(power: &mut PowerMetrics, gpu: &GpuMetrics) {
    power.gpu_board_w = gpu.power_w.clone();
    power.gpu_temperature_c = gpu.temperature_c.clone();
    power.fan_pct = gpu.fan_pct.clone();
    power.power_limit_pct = gpu.power_limit_pct.clone();
    let usable = |reading: &Reading<f64>| {
        matches!(reading.status, ReadingStatus::Valid | ReadingStatus::Stale)
            && reading.value.is_some()
    };
    if usable(&power.battery_charge_pct) {
        power.primary_kind = PowerPrimaryKind::BatteryPct;
        power.primary_value = power.battery_charge_pct.clone();
        if power.battery_state.value != Some(BatteryState::Discharging)
            || !matches!(
                power.battery_state.status,
                ReadingStatus::Valid | ReadingStatus::Stale
            )
        {
            power.watts = Reading::unavailable(
                power
                    .battery_charge_pct
                    .scope_id
                    .as_deref()
                    .unwrap_or("battery"),
                "Battery discharge is unavailable while on AC",
            );
        }
    } else if usable(&gpu.power_w) {
        power.primary_kind = PowerPrimaryKind::GpuWatts;
        power.primary_value = gpu.power_w.clone();
        power.watts = gpu.power_w.clone();
    } else if usable(&power.cpu_package_w) {
        power.primary_kind = PowerPrimaryKind::CpuPackageWatts;
        power.primary_value = power.cpu_package_w.clone();
        power.watts = power.cpu_package_w.clone();
    } else {
        power.primary_kind = PowerPrimaryKind::Unavailable;
        power.primary_value =
            Reading::unavailable("power", "No measured power source is available");
        power.primary_value.scope_id = None;
        power.watts = power.primary_value.clone();
    }
}

fn prefer<T: Clone>(target: &mut Reading<T>, candidate: &Reading<T>) {
    if target.status == ReadingStatus::Valid {
        return;
    }
    if candidate.status == ReadingStatus::Valid
        || matches!(
            candidate.status,
            ReadingStatus::PermissionDenied | ReadingStatus::Unsupported
        )
        || (candidate.status == ReadingStatus::Unavailable
            && target.status == ReadingStatus::Unavailable)
    {
        *target = candidate.clone();
    }
}
/// Owns the last valid observation of each field across transient failures before the stale
/// threshold. A single previous published unavailable sample is insufficient for this purpose.
#[derive(Default)]
pub struct Normalizer {
    last_valid: Option<CurrentSample>,
}
impl Normalizer {
    pub fn normalize(&mut self, frame: CurrentSample, polls: &[ProviderPoll]) -> CurrentSample {
        let output = merge(frame, polls, self.last_valid.as_ref());
        let mut cache = output.clone();
        if let Some(previous) = &self.last_valid {
            macro_rules! cache_missing {
                ($group:ident, $( $field:ident ),+ $(,)?) => {$(
                    if output.$group.$field.status == ReadingStatus::Unavailable
                        && same_epoch(previous, &output, output.$group.$field.scope_id.as_deref())
                        && output.$group.$field.scope_id == previous.$group.$field.scope_id
                    { cache.$group.$field = previous.$group.$field.clone(); }
                )+};
            }
            cache_missing!(cpu, total_pct, user_pct, system_pct, temperature_c);
            cache_missing!(
                memory,
                total_bytes,
                used_bytes,
                available_bytes,
                cached_bytes,
                used_pct
            );
            cache_missing!(
                gpu,
                utilization_pct,
                vram_total_bytes,
                vram_used_bytes,
                vram_free_bytes,
                vram_reserved_bytes,
                temperature_c,
                power_w,
                fan_pct,
                power_limit_pct
            );
            cache_missing!(
                disk,
                total_bytes,
                free_bytes,
                read_bytes_per_second,
                write_bytes_per_second,
                written_session_bytes
            );
            cache_missing!(
                network,
                receive_bytes_per_second,
                send_bytes_per_second,
                received_session_bytes,
                sent_session_bytes
            );
            cache_missing!(
                power,
                primary_value,
                watts,
                cpu_package_w,
                gpu_board_w,
                gpu_temperature_c,
                fan_pct,
                power_limit_pct,
                battery_charge_pct,
                battery_health_pct,
                battery_remaining_seconds,
                battery_state
            );
        }
        self.last_valid = Some(cache);
        output
    }
    pub fn reset(&mut self) {
        self.last_valid = None;
    }
}
