//! Independent RAM and dedicated-VRAM breakdowns. RAM and VRAM never share a denominator.
use crate::telemetry::model::*;

fn valid<T>(reading: &Reading<T>) -> bool {
    reading.status == ReadingStatus::Valid && reading.value.is_some()
}

fn segment(id: &str, label: &str, bytes: u64, role: SegmentRole) -> MemorySegment {
    MemorySegment {
        id: id.into(),
        label: label.into(),
        bytes: ByteCount(bytes),
        role,
    }
}

/// RAM is an exact In Use/Available partition; cache stays outside the donut.
pub fn ram(memory: &MemoryMetrics) -> Option<MemoryBreakdown> {
    if !valid(&memory.total_bytes)
        || !valid(&memory.available_bytes)
        || !valid(&memory.used_bytes)
        || !valid(&memory.used_pct)
    {
        return None;
    }
    let total = memory.total_bytes.value?.0;
    let used = memory.used_bytes.value?.0;
    let available = memory.available_bytes.value?.0;
    if total == 0 || used.checked_add(available)? != total {
        return None;
    }
    Some(MemoryBreakdown {
        kind: MemoryView::Ram,
        scope_id: memory.total_bytes.scope_id.clone()?,
        total_bytes: ByteCount(total),
        used_pct: memory.used_pct.value?,
        segments: vec![
            segment("ram-used", "In Use", used, SegmentRole::Used),
            segment(
                "ram-available",
                "Available",
                available,
                SegmentRole::Available,
            ),
        ],
        coverage: BreakdownCoverage::Complete,
        note: Some("Cache overlaps available RAM and is informational".into()),
    })
}

/// Dedicated VRAM uses provider-reported disjoint categories. Unknown capacity becomes an honest
/// Unclassified residual; overfull category sets are rejected instead of scaled.
pub fn vram(gpu: &GpuMetrics) -> Option<MemoryBreakdown> {
    let total = gpu
        .vram_total_bytes
        .value
        .filter(|_| valid(&gpu.vram_total_bytes))
        .map(|value| value.0)?;
    let used = gpu
        .vram_used_bytes
        .value
        .filter(|_| valid(&gpu.vram_used_bytes))
        .map(|value| value.0)?;
    if total == 0 || used > total {
        return None;
    }
    let scope_id = gpu.vram_total_bytes.scope_id.clone()?;
    let used_pct = 100.0 * used as f64 / total as f64;
    if !valid_pct(&used_pct) {
        return None;
    }
    let mut segments = vec![segment("vram-used", "Used", used, SegmentRole::Used)];
    let mut covered = used;
    if valid(&gpu.vram_free_bytes)
        && let Some(free) = gpu.vram_free_bytes.value.map(|value| value.0)
    {
        if covered.checked_add(free)? > total {
            return None;
        }
        covered += free;
        segments.push(segment("vram-free", "Free", free, SegmentRole::Free));
    }
    if valid(&gpu.vram_reserved_bytes)
        && let Some(reserved) = gpu.vram_reserved_bytes.value.map(|value| value.0)
    {
        if covered.checked_add(reserved)? > total {
            return None;
        }
        covered += reserved;
        segments.push(segment(
            "vram-reserved",
            "Reserved",
            reserved,
            SegmentRole::Reserved,
        ));
    }
    let coverage = if covered == total {
        BreakdownCoverage::Complete
    } else {
        segments.push(segment(
            "vram-unclassified",
            "Unclassified",
            total - covered,
            SegmentRole::Unclassified,
        ));
        BreakdownCoverage::Partial
    };
    Some(MemoryBreakdown {
        kind: MemoryView::Vram,
        scope_id,
        total_bytes: ByteCount(total),
        used_pct,
        segments,
        coverage,
        note: Some(if coverage == BreakdownCoverage::Partial {
            "Some dedicated-VRAM categories are not reported; Unclassified is not free memory"
                .into()
        } else {
            "Dedicated VRAM excludes shared system RAM".into()
        }),
    })
}

pub fn breakdowns(memory: &MemoryMetrics, gpu: &GpuMetrics) -> MemoryBreakdowns {
    MemoryBreakdowns {
        ram: ram(memory),
        vram: vram(gpu),
    }
}
