//! Serialized shared state and typed command outcomes for the single main window.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::telemetry::bounds;
use crate::telemetry::model::{MemoryView, Mode, Range, RankingMetric, ViewSelection};

pub const CONTROL_QUEUE_CAPACITY: usize = 32;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode {
    InvalidArgument,
    UnknownGpu,
    ResumeRequired,
    UnsupportedRanking,
    InvalidSubscription,
    Busy,
    NotReady,
    PayloadTooLarge,
    Internal,
}

impl CommandErrorCode {
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Busy | Self::NotReady | Self::Internal)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl CommandError {
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: bounds::bounded_message(&message.into()),
            retryable: code.is_retryable(),
        }
    }

    pub fn busy() -> Self {
        Self::new(
            CommandErrorCode::Busy,
            "The control queue is full; retry the action",
        )
    }
}

/// `{ok:true,data:T}` or `{ok:false,error:{code,message,retryable}}`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
}

impl<T> CommandResult<T> {
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn failure(error: CommandError) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    pub fn from_result(result: Result<T, CommandError>) -> Self {
        match result {
            Ok(data) => Self::ok(data),
            Err(error) => Self::failure(error),
        }
    }
}

/// Bounded admission control for control commands; admission never grows with uptime.
#[derive(Clone, Debug)]
pub struct ControlQueue {
    depth: Arc<AtomicUsize>,
    capacity: usize,
}

impl Default for ControlQueue {
    fn default() -> Self {
        Self::with_capacity(CONTROL_QUEUE_CAPACITY)
    }
}

impl ControlQueue {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            depth: Arc::new(AtomicUsize::new(0)),
            capacity,
        }
    }

    pub fn try_acquire(&self) -> Result<ControlPermit, CommandError> {
        let mut current = self.depth.load(Ordering::Acquire);
        loop {
            if current >= self.capacity {
                return Err(CommandError::busy());
            }
            match self.depth.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(ControlPermit {
                        queue: self.clone(),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    pub fn depth(&self) -> usize {
        self.depth.load(Ordering::Acquire)
    }
}

pub struct ControlPermit {
    queue: ControlQueue,
}

impl Drop for ControlPermit {
    fn drop(&mut self) {
        self.queue.depth.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewState {
    pub revision: u64,
    pub range: Range,
    pub memory_view: MemoryView,
    pub ranking_metric: RankingMetric,
    pub mode: Mode,
    pub visible: bool,
    pub inspection_id: Option<String>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            revision: 0,
            range: Range::OneMinute,
            memory_view: MemoryView::Ram,
            ranking_metric: RankingMetric::Cpu,
            mode: Mode::Live,
            visible: true,
            inspection_id: None,
        }
    }
}

impl ViewState {
    pub fn selection(&self) -> ViewSelection {
        ViewSelection {
            range: self.range,
            memory_view: self.memory_view,
            ranking_metric: self.ranking_metric,
        }
    }

    /// Applies a complete selection update; returns whether anything changed.
    pub fn apply_selection(&mut self, selection: ViewSelection) -> bool {
        let changed = self.selection() != selection;
        self.range = selection.range;
        self.memory_view = selection.memory_view;
        self.ranking_metric = selection.ranking_metric;
        changed
    }

    pub fn apply_visibility(&mut self, visible: bool) -> bool {
        let changed = self.visible != visible;
        self.visible = visible;
        changed
    }

    /// Pause captures one frozen inspection; Resume discards it. Repeating either is a no-op.
    pub fn apply_paused(&mut self, paused: bool) -> bool {
        if paused == (self.mode == Mode::Paused) {
            return false;
        }
        if paused {
            self.mode = Mode::Paused;
            self.inspection_id = Some(uuid::Uuid::new_v4().to_string());
        } else {
            self.mode = Mode::Live;
            self.inspection_id = None;
        }
        true
    }
}

/// Owns the single mutable view state and control admission. All mutations are serialized.
#[derive(Debug, Default)]
pub struct StateOwner {
    state: Mutex<ViewState>,
    controls: ControlQueue,
}

impl StateOwner {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, ViewState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> ViewState {
        self.state().clone()
    }

    pub fn acquire_control(&self) -> Result<ControlPermit, CommandError> {
        self.controls.try_acquire()
    }

    /// No-op selections preserve the revision; effective changes increment it once.
    pub fn apply_selection(&self, selection: ViewSelection) -> CommandResult<ViewState> {
        let _permit = match self.controls.try_acquire() {
            Ok(permit) => permit,
            Err(error) => return CommandResult::failure(error),
        };
        let mut state = self.state();
        if state.apply_selection(selection) {
            state.revision = state.revision.saturating_add(1);
        }
        CommandResult::ok(state.clone())
    }

    pub fn apply_visibility(&self, visible: bool) -> CommandResult<ViewState> {
        let _permit = match self.controls.try_acquire() {
            Ok(permit) => permit,
            Err(error) => return CommandResult::failure(error),
        };
        let mut state = self.state();
        if state.apply_visibility(visible) {
            state.revision = state.revision.saturating_add(1);
        }
        CommandResult::ok(state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn selection(
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

    #[test]
    fn view_state_starts_live_visible_at_one_minute_ram_cpu() {
        let state = ViewState::default();
        assert_eq!(state.revision, 0);
        assert_eq!(state.range, Range::OneMinute);
        assert_eq!(state.memory_view, MemoryView::Ram);
        assert_eq!(state.ranking_metric, RankingMetric::Cpu);
        assert_eq!(state.mode, Mode::Live);
        assert!(state.visible);
        assert!(state.inspection_id.is_none());
    }

    #[test]
    fn no_op_selection_retains_revision_and_changes_increment_once() {
        let owner = StateOwner::new();
        let unchanged = owner.apply_selection(selection(
            Range::OneMinute,
            MemoryView::Ram,
            RankingMetric::Cpu,
        ));
        assert!(unchanged.ok);
        assert_eq!(unchanged.data.unwrap().revision, 0);

        let changed = owner.apply_selection(selection(
            Range::FiveMinutes,
            MemoryView::Vram,
            RankingMetric::Gpu,
        ));
        let state = changed.data.unwrap();
        assert_eq!(state.revision, 1);
        assert_eq!(state.range, Range::FiveMinutes);
        assert_eq!(state.memory_view, MemoryView::Vram);
        assert_eq!(state.ranking_metric, RankingMetric::Gpu);

        let repeated = owner.apply_selection(state.selection());
        assert_eq!(repeated.data.unwrap().revision, 1);

        let hidden = owner.apply_visibility(false);
        assert_eq!(hidden.data.unwrap().revision, 2);
        assert_eq!(owner.apply_visibility(false).data.unwrap().revision, 2);
    }

    #[test]
    fn control_queue_accepts_thirty_two_operations_then_reports_busy() {
        let queue = ControlQueue::default();
        let permits: Vec<_> = (0..CONTROL_QUEUE_CAPACITY)
            .map(|_| queue.try_acquire().expect("capacity must admit 32"))
            .collect();
        assert_eq!(queue.depth(), CONTROL_QUEUE_CAPACITY);
        let error = queue
            .try_acquire()
            .err()
            .expect("overflow must be rejected");
        assert_eq!(error.code, CommandErrorCode::Busy);
        assert!(error.retryable);

        drop(permits);
        assert_eq!(queue.depth(), 0);
        assert!(queue.try_acquire().is_ok());
    }

    #[test]
    fn command_results_serialize_the_contracted_envelopes() {
        let ok = serde_json::to_value(CommandResult::ok(7)).unwrap();
        assert_eq!(ok, serde_json::json!({"ok": true, "data": 7}));

        let failure = CommandResult::<u8>::failure(CommandError::new(
            CommandErrorCode::InvalidArgument,
            "range must be one of 1m, 5m, 15m or 1h",
        ));
        let wire = serde_json::to_value(&failure).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "ok": false,
                "error": {
                    "code": "invalid_argument",
                    "message": "range must be one of 1m, 5m, 15m or 1h",
                    "retryable": false
                }
            })
        );
    }
}
