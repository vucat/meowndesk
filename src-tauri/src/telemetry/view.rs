//! Complete coherent Views and truthful breakdown content.
use crate::telemetry::{
    bounds, history::HistoryStore, memory, model::*, projection::project, state::ViewState,
};
pub fn initializing(session_id: String) -> View {
    View {
        contract_version: CONTRACT_VERSION,
        session_id,
        revision: 0,
        mode: Mode::Live,
        visible: true,
        inspection_id: None,
        endpoint_elapsed_ms: 0,
        endpoint_at_ms: 0,
        sample: None,
        process_summaries: None,
        memory_breakdowns: MemoryBreakdowns {
            ram: None,
            vram: None,
        },
        history: None,
        selection: ViewState::default().selection(),
        preferences: CollectionPreferences {
            interval_ms: 1000,
            selected_gpu_id: None,
        },
        notices: vec![],
    }
}
pub fn build(
    state: &ViewState,
    sample: CurrentSample,
    history: &HistoryStore,
    processes: &[ProcessObservation],
    preferences: CollectionPreferences,
    mut notices: Vec<Notice>,
) -> View {
    let projection = project(history, sample.elapsed_ms, state.range, &sample);
    let gpu_scope = sample
        .scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Gpu)
        .map(|scope| scope.id.as_str());
    let summaries = crate::telemetry::processes::summarize(processes, gpu_scope);
    let breakdowns = memory::breakdowns(&sample.memory, &sample.gpu);
    let mut selection = state.selection();
    if selection.ranking_metric != RankingMetric::Cpu
        && !summaries
            .supported_ranking_metrics
            .contains(&selection.ranking_metric)
    {
        selection.ranking_metric = RankingMetric::Cpu;
        notices.push(Notice {
            code: "ranking_fallback".into(),
            message: "The selected ranking is unavailable; showing CPU".into(),
        });
    }
    notices.truncate(bounds::MAX_NOTICES);
    View {
        contract_version: CONTRACT_VERSION,
        session_id: sample.host.session_id.clone(),
        revision: state.revision,
        mode: state.mode,
        visible: state.visible,
        inspection_id: state.inspection_id.clone(),
        endpoint_elapsed_ms: sample.elapsed_ms,
        endpoint_at_ms: sample.captured_at_ms,
        process_summaries: Some(summaries),
        memory_breakdowns: breakdowns,
        history: Some(projection),
        selection,
        preferences,
        notices,
        sample: Some(sample),
    }
}
/// Rebuilds the active source (live or frozen) for a new selection/endpoint without touching it.
pub fn reselect(
    state: &ViewState,
    base: &View,
    history: &HistoryStore,
    preferences: CollectionPreferences,
    visible: bool,
) -> Option<View> {
    let sample = base.sample.as_ref()?;
    let mut view = base.clone();
    view.revision = state.revision;
    view.mode = state.mode;
    view.visible = visible;
    view.inspection_id = state.inspection_id.clone();
    let mut selection = state.selection();
    if selection.ranking_metric != RankingMetric::Cpu
        && !view.process_summaries.as_ref().is_some_and(|summaries| {
            summaries
                .supported_ranking_metrics
                .contains(&selection.ranking_metric)
        })
    {
        selection.ranking_metric = RankingMetric::Cpu;
        if !view
            .notices
            .iter()
            .any(|notice| notice.code == "ranking_fallback")
        {
            view.notices.push(Notice {
                code: "ranking_fallback".into(),
                message: "The selected ranking is unavailable; showing CPU".into(),
            });
            view.notices.truncate(bounds::MAX_NOTICES);
        }
    }
    view.selection = selection;
    view.preferences = preferences;
    view.history = Some(project(history, sample.elapsed_ms, state.range, sample));
    Some(view)
}
