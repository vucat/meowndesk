//! Per-identity interface deltas. Only metadata is shortened; every eligible device is sampled.
use crate::telemetry::{
    bounds::{NetworkMemberMetadata, network_member_metadata},
    model::*,
    normalize::empty_network,
    providers::*,
    rates::{CounterBaseline, RateOutcome},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
pub const NETWORK_SCOPE: &str = "network:interface-traffic";
#[derive(Clone, Debug)]
pub struct InterfaceCounters {
    pub id: String,
    pub up: bool,
    pub loopback: bool,
    pub received: u64,
    pub sent: u64,
}
#[derive(Default)]
struct InterfaceState {
    received: CounterBaseline,
    sent: CounterBaseline,
}
#[derive(Default)]
pub struct NetworkTracker {
    interfaces: BTreeMap<String, InterfaceState>,
    received: u64,
    sent: u64,
}
pub struct NetworkObservation {
    pub metrics: NetworkMetrics,
    pub members: NetworkMemberMetadata,
}
impl NetworkTracker {
    pub fn observe(
        &mut self,
        interfaces: Result<Vec<InterfaceCounters>, ProviderError>,
        elapsed: u64,
        interval: u64,
        wall: i64,
    ) -> NetworkObservation {
        let mut metrics = empty_network(NETWORK_SCOPE);
        let interfaces = match interfaces {
            Ok(interfaces) => interfaces,
            Err(error) => {
                self.interfaces.clear();
                metrics.receive_bytes_per_second = error.reading(
                    NETWORK_SCOPE,
                    "Interface discovery or counters are not readable",
                );
                metrics.send_bytes_per_second = metrics.receive_bytes_per_second.clone();
                return NetworkObservation {
                    metrics,
                    members: network_member_metadata(&[]),
                };
            }
        };
        let eligible: BTreeMap<_, _> = interfaces
            .into_iter()
            .filter(|i| i.up && !i.loopback)
            .map(|i| (i.id.clone(), i))
            .collect();
        let ids: Vec<_> = eligible.keys().cloned().collect();
        let active: BTreeSet<_> = ids.iter().cloned().collect();
        self.interfaces.retain(|id, _| active.contains(id));
        let mut receive = 0.0;
        let mut send = 0.0;
        let mut complete = true;
        for (id, interface) in eligible {
            let state = self.interfaces.entry(id).or_default();
            let r = state
                .received
                .observe(interface.received, elapsed, interval);
            let s = state.sent.observe(interface.sent, elapsed, interval);
            match (r, r.per_second()) {
                (RateOutcome::Sample { delta, .. }, Some(rate)) => {
                    self.received = self.received.saturating_add(delta);
                    receive += rate;
                }
                _ => complete = false,
            }
            match (s, s.per_second()) {
                (RateOutcome::Sample { delta, .. }, Some(rate)) => {
                    self.sent = self.sent.saturating_add(delta);
                    send += rate;
                }
                _ => complete = false,
            }
        }
        if complete && receive.is_finite() && send.is_finite() {
            metrics.receive_bytes_per_second =
                Reading::valid(receive, wall, elapsed, NETWORK_SCOPE);
            metrics.send_bytes_per_second = Reading::valid(send, wall, elapsed, NETWORK_SCOPE);
        } else {
            metrics.receive_bytes_per_second = Reading::unavailable(
                NETWORK_SCOPE,
                "Partial interface coverage; new or reset counters are warming up",
            );
            metrics.send_bytes_per_second = metrics.receive_bytes_per_second.clone();
        }
        metrics.received_session_bytes =
            Reading::valid(ByteCount(self.received), wall, elapsed, NETWORK_SCOPE);
        metrics.sent_session_bytes =
            Reading::valid(ByteCount(self.sent), wall, elapsed, NETWORK_SCOPE);
        NetworkObservation {
            metrics,
            members: network_member_metadata(&ids),
        }
    }
}
#[derive(Default)]
pub struct NetworkProvider {
    tracker: Mutex<NetworkTracker>,
}
impl Provider for NetworkProvider {
    fn provider_id(&self) -> &'static str {
        "network"
    }
    fn poll(&self, ctx: &PollContext<'_>) -> ProviderPoll {
        #[cfg(target_os = "linux")]
        let interfaces = super::linux::network::interfaces();
        #[cfg(target_os = "windows")]
        let interfaces = super::windows::network::interfaces();
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let interfaces = Err(ProviderError::Unsupported);
        let observation = self
            .tracker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(
                interfaces,
                ctx.elapsed_ms,
                ctx.interval_ms,
                ctx.sampled_at_ms(),
            );
        ProviderPoll {
            network: Some(observation.metrics),
            network_members: Some(observation.members),
            ..ProviderPoll::default()
        }
    }
}
