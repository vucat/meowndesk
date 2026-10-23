//! Stable scope identity, epoch bookkeeping and 30-second rediscovery.

use std::collections::BTreeMap;

use crate::telemetry::model::{GpuDescriptor, GpuKind, GpuVendor, ResourceScope, ScopeKind};

pub const DISCOVERY_INTERVAL_MS: u64 = 30_000;

/// Identity classes that survive reboot. Windows runtime LUID is never one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuIdentityKind {
    NvmlUuid,
    WindowsPnp,
    LinuxPci,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuCandidate {
    pub descriptor: GpuDescriptor,
    pub identity_kind: GpuIdentityKind,
}

impl GpuCandidate {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        vendor: GpuVendor,
        kind: GpuKind,
        available: bool,
        identity_kind: GpuIdentityKind,
    ) -> Self {
        Self {
            descriptor: GpuDescriptor {
                id: id.into(),
                name: name.into(),
                vendor,
                kind,
                available,
            },
            identity_kind,
        }
    }
}

/// Selection order: saved valid identity, NVIDIA discrete, other discrete, integrated, then
/// stable-ID order for ties.
pub fn select_gpu<'a>(
    candidates: &'a [GpuCandidate],
    saved_id: Option<&str>,
) -> Option<&'a GpuCandidate> {
    if let Some(saved_id) = saved_id
        && let Some(saved) = candidates
            .iter()
            .find(|candidate| candidate.descriptor.id == saved_id)
    {
        return Some(saved);
    }
    candidates.iter().min_by(|left, right| {
        selection_rank(left)
            .cmp(&selection_rank(right))
            .then_with(|| left.descriptor.id.cmp(&right.descriptor.id))
    })
}

fn selection_rank(candidate: &GpuCandidate) -> u8 {
    match (candidate.descriptor.vendor, candidate.descriptor.kind) {
        (GpuVendor::Nvidia, GpuKind::Discrete) => 0,
        (_, GpuKind::Discrete) => 1,
        (_, GpuKind::Integrated) => 2,
        _ => 3,
    }
}

/// Milliseconds to whole seconds; `None` stays `None` so an unavailable uptime is never 0.
pub fn uptime_seconds(uptime_ms: Option<u64>) -> Option<u64> {
    uptime_ms.map(|milliseconds| milliseconds / 1000)
}

#[derive(Clone, Debug, Default)]
pub struct ScopeRegistry {
    scopes: BTreeMap<String, ResourceScope>,
    last_discovery_ms: Option<u64>,
}

impl ScopeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a scope. Existing ids keep their epoch; only an explicit source change bumps it.
    pub fn upsert(
        &mut self,
        id: impl Into<String>,
        kind: ScopeKind,
        label: impl Into<String>,
        provider: impl Into<String>,
    ) -> &ResourceScope {
        let id = id.into();
        let scope = self.scopes.entry(id.clone()).or_insert(ResourceScope {
            id,
            kind,
            label: String::new(),
            provider: String::new(),
            epoch: 1,
        });
        scope.kind = kind;
        scope.label = label.into();
        scope.provider = provider.into();
        scope
    }

    pub fn bump_epoch(&mut self, id: &str) -> Option<u64> {
        let scope = self.scopes.get_mut(id)?;
        scope.epoch = scope.epoch.saturating_add(1);
        Some(scope.epoch)
    }

    pub fn set_epoch(&mut self, id: &str, epoch: u64) {
        if let Some(scope) = self.scopes.get_mut(id) {
            scope.epoch = epoch;
        }
    }

    pub fn get(&self, id: &str) -> Option<&ResourceScope> {
        self.scopes.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<ResourceScope> {
        self.scopes.remove(id)
    }

    /// Removes scopes that discovery no longer reports and returns the removed records.
    pub fn reconcile(&mut self, seen_ids: &[String]) -> Vec<ResourceScope> {
        let stale: Vec<String> = self
            .scopes
            .keys()
            .filter(|id| !seen_ids.iter().any(|seen| seen == *id))
            .cloned()
            .collect();
        stale
            .into_iter()
            .filter_map(|id| self.scopes.remove(&id))
            .collect()
    }

    pub fn all(&self) -> Vec<ResourceScope> {
        self.scopes.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    pub fn mark_discovered(&mut self, elapsed_ms: u64) {
        self.last_discovery_ms = Some(elapsed_ms);
    }

    pub fn discovery_due(&self, elapsed_ms: u64, provider_lost: bool) -> bool {
        provider_lost
            || self
                .last_discovery_ms
                .is_none_or(|last| elapsed_ms.saturating_sub(last) >= DISCOVERY_INTERVAL_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, vendor: GpuVendor, kind: GpuKind) -> GpuCandidate {
        GpuCandidate::new(id, id, vendor, kind, true, GpuIdentityKind::NvmlUuid)
    }

    #[test]
    fn saved_identity_wins_and_stable_order_breaks_ties() {
        let candidates = vec![
            candidate("pci-b", GpuVendor::Intel, GpuKind::Integrated),
            candidate("pci-a", GpuVendor::Amd, GpuKind::Discrete),
            candidate("gpu-nv", GpuVendor::Nvidia, GpuKind::Discrete),
            candidate("pci-c", GpuVendor::Other, GpuKind::Other),
        ];
        assert_eq!(
            select_gpu(&candidates, Some("pci-b"))
                .unwrap()
                .descriptor
                .id,
            "pci-b"
        );
        assert_eq!(
            select_gpu(&candidates, None).unwrap().descriptor.id,
            "gpu-nv"
        );
        assert_eq!(
            select_gpu(&candidates, Some("missing"))
                .unwrap()
                .descriptor
                .id,
            "gpu-nv"
        );

        let ties = vec![
            candidate("b", GpuVendor::Amd, GpuKind::Discrete),
            candidate("a", GpuVendor::Amd, GpuKind::Discrete),
        ];
        assert_eq!(select_gpu(&ties, None).unwrap().descriptor.id, "a");
        assert!(select_gpu(&[], None).is_none());
    }

    #[test]
    fn epochs_change_only_on_explicit_source_change() {
        let mut registry = ScopeRegistry::new();
        registry.upsert("gpu:1", ScopeKind::Gpu, "GPU", "nvml");
        assert_eq!(registry.get("gpu:1").unwrap().epoch, 1);
        registry.upsert("gpu:1", ScopeKind::Gpu, "GPU renamed", "nvml");
        assert_eq!(registry.get("gpu:1").unwrap().epoch, 1);
        assert_eq!(registry.bump_epoch("gpu:1"), Some(2));
        assert_eq!(registry.bump_epoch("missing"), None);

        let removed = registry.reconcile(&[]);
        assert_eq!(removed.len(), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn discovery_runs_every_thirty_seconds_or_after_provider_loss() {
        let mut registry = ScopeRegistry::new();
        assert!(registry.discovery_due(0, false));
        registry.mark_discovered(5_000);
        assert!(!registry.discovery_due(34_999, false));
        assert!(registry.discovery_due(35_000, false));
        assert!(registry.discovery_due(35_001, true));
    }

    #[test]
    fn unavailable_uptime_is_not_zero() {
        assert_eq!(uptime_seconds(None), None);
        assert_eq!(uptime_seconds(Some(128_640_000)), Some(128_640));
    }
}
