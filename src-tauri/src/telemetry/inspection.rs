//! One frozen inspection source: a coherent endpoint plus its bounded scalar ring.
//!
//! At most two histories exist in a session (live and this optional copy). Repeated Pause reuses
//! the same source and Resume drops it; ranges never own a ring of their own.
use crate::telemetry::{history::HistoryStore, model::View};

#[derive(Clone, Debug)]
pub struct FrozenSource {
    pub id: String,
    /// Snapshot of the live view: sample, bounded top lists, capabilities/breakdowns and notices.
    pub view: View,
    pub history: HistoryStore,
}

impl FrozenSource {
    /// Copies warm-up/partial state as-is. A session without any sample cannot be paused.
    pub fn capture(id: String, live: &View, history: &HistoryStore) -> Option<Self> {
        live.sample.as_ref()?;
        Some(Self {
            id,
            view: live.clone(),
            history: history.clone(),
        })
    }
}
