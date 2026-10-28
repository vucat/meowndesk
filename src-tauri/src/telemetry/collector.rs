//! One collection worker, one serialized publication/control owner, and cached coherent views.
//! Sensor I/O runs outside the owner lock on a dedicated blocking thread. Tokio skips missed ticks.
use crate::telemetry::{
    baseline, bounds,
    clock::{SessionClock, SystemClock},
    delivery::{DeliveryState, Sender},
    history::{HistoryStore, ScalarFrame},
    inspection::FrozenSource,
    model::*,
    normalize::{self, Normalizer},
    providers::{self, PollContext, Provider, ProviderPoll},
    scopes::{GpuCandidate, GpuIdentityKind, ScopeRegistry, select_gpu},
    state::{CommandError, CommandErrorCode, ControlPermit, ControlQueue, ViewState},
    view,
};
#[cfg(feature = "test-fixtures")]
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct PollStamp {
    pub gpu_id: Option<String>,
    pub gpu_epoch: u64,
    pub power_source: Option<String>,
    pub power_epoch: u64,
    pub interval_ms: u64,
}
struct Core {
    state: ViewState,
    /// Latest live-built view; kept collecting while a frozen source is displayed.
    live: View,
    /// What the frontend currently receives: the live view or the frozen inspection.
    cached: View,
    frozen: Option<FrozenSource>,
    gpus: Vec<GpuDescriptor>,
    history: HistoryStore,
    scopes: ScopeRegistry,
    normalizer: Normalizer,
    /// Configuration notices survive telemetry ticks until a successful preference save.
    session_notices: Vec<Notice>,
    preferences_path: Option<PathBuf>,
    /// Saved choice stays private until background discovery establishes a catalog.
    initial_gpu_id: Option<String>,
    gpu_epoch: u64,
    power_epoch: u64,
    power_source: Option<String>,
    native_visible: bool,
    closed: bool,
    delivery: DeliveryState,
    #[cfg(feature = "test-fixtures")]
    fixture_installed: bool,
}
pub struct TelemetryService {
    inner: Mutex<Core>,
    controls: ControlQueue,
    stopping: Arc<AtomicBool>,
    wake: Arc<tokio::sync::Notify>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl Default for TelemetryService {
    fn default() -> Self {
        Self::new()
    }
}
impl TelemetryService {
    pub fn new() -> Self {
        let initial = view::initializing(uuid::Uuid::new_v4().to_string());
        Self {
            inner: Mutex::new(Core {
                state: ViewState::default(),
                live: initial.clone(),
                cached: initial,
                frozen: None,
                gpus: vec![],
                history: HistoryStore::default(),
                scopes: ScopeRegistry::new(),
                normalizer: Normalizer::default(),
                session_notices: Vec::with_capacity(3),
                preferences_path: None,
                initial_gpu_id: None,
                gpu_epoch: 1,
                power_epoch: 1,
                power_source: None,
                native_visible: true,
                closed: false,
                delivery: DeliveryState::default(),
                #[cfg(feature = "test-fixtures")]
                fixture_installed: false,
            }),
            controls: ControlQueue::default(),
            stopping: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(tokio::sync::Notify::new()),
            worker: Mutex::new(None),
        }
    }
    fn core(&self) -> std::sync::MutexGuard<'_, Core> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn admit(&self) -> Result<ControlPermit, CommandError> {
        self.controls.try_acquire()
    }
    pub fn bootstrap(&self) -> Bootstrap {
        let c = self.core();
        Bootstrap {
            view: c.cached.clone(),
            gpus: c.gpus.clone(),
        }
    }
    pub fn capture_poll(&self) -> PollStamp {
        let c = self.core();
        PollStamp {
            gpu_id: c.live.preferences.selected_gpu_id.clone(),
            gpu_epoch: c.gpu_epoch,
            power_source: c.power_source.clone(),
            power_epoch: c.power_epoch,
            interval_ms: c.live.preferences.interval_ms,
        }
    }
    /// Loads validated launch preferences and remembers the fixed file path for later saves.
    pub fn initialize_preferences(
        &self,
        path: PathBuf,
        mut preferences: CollectionPreferences,
        notice: Option<String>,
    ) {
        let mut c = self.core();
        c.preferences_path = Some(path);
        c.initial_gpu_id = preferences.selected_gpu_id.take();
        c.live.preferences = preferences.clone();
        c.cached.preferences = preferences;
        if let Some(message) = notice {
            c.set_session_notice(Notice {
                code: "preferences_reset".into(),
                message: bounds::bounded_message(&message),
            });
        }
    }
    /// Validates the complete request before mutating. A failed write keeps in-session choices.
    pub fn set_preferences(
        &self,
        interval_ms: u64,
        selected_gpu_id: Option<String>,
    ) -> Result<(View, bool), CommandError> {
        let mut c = self.core();
        if c.closed {
            return Err(not_ready());
        }
        let next = CollectionPreferences {
            interval_ms,
            selected_gpu_id,
        };
        if !crate::preferences::validate(&next) {
            return Err(CommandError::new(
                CommandErrorCode::InvalidArgument,
                "intervalMs must be 1000, 2000 or 5000 with a valid GPU identity",
            ));
        }
        if let Some(id) = &next.selected_gpu_id {
            if !c.gpus.iter().any(|gpu| &gpu.id == id) {
                return Err(CommandError::new(
                    CommandErrorCode::UnknownGpu,
                    "The selected GPU is no longer discovered",
                ));
            }
        } else if !c.gpus.is_empty() {
            return Err(CommandError::new(
                CommandErrorCode::UnknownGpu,
                "A discovered GPU must be selected",
            ));
        }
        let current = c.live.preferences.clone();
        let gpu_changed = next.selected_gpu_id != current.selected_gpu_id;
        let interval_changed = next.interval_ms != current.interval_ms;
        if c.state.mode == Mode::Paused && gpu_changed {
            return Err(CommandError::new(
                CommandErrorCode::ResumeRequired,
                "Resume before changing the monitored GPU",
            ));
        }
        c.live.preferences = next.clone();
        c.cached.preferences = next.clone();
        if gpu_changed {
            let chosen = c
                .gpus
                .iter()
                .find(|gpu| Some(&gpu.id) == next.selected_gpu_id.as_ref())
                .cloned();
            c.rebind_gpu(chosen);
        } else if interval_changed {
            c.state.revision = c.state.revision.saturating_add(1);
            c.refresh_control();
            c.delivery_mut().clear_pending();
        }
        let mut persisted = true;
        if let Some(path) = c.preferences_path.clone()
            && crate::preferences::save(&path, &next).is_err()
        {
            persisted = false;
            c.set_session_notice(Notice {
                code: "preferences_not_saved".into(),
                message: "Preferences could not be saved; in-session choices are still active"
                    .into(),
            });
        } else {
            // Saving acknowledges repaired preferences and the currently selected adapter.
            c.session_notices.clear();
            c.refresh_session_notices();
        }
        let view = c.cached.clone();
        drop(c);
        if interval_changed {
            self.wake.notify_one();
        }
        Ok((view, persisted))
    }
    pub fn subscribe(&self, sender: Sender) -> Result<(String, String), CommandError> {
        let mut c = self.core();
        if c.closed {
            return Err(not_ready());
        }
        let id = c.delivery_mut().subscribe(sender);
        Ok((id, c.cached.session_id.clone()))
    }
    pub fn ack(&self, id: &str, sequence: u64) -> Result<bool, CommandError> {
        let mut c = self.core();
        let latest = c.cached.clone();
        let allowed = c.allowed();
        c.delivery_mut().ack(id, sequence, &latest, allowed)
    }
    pub fn unsubscribe(&self, id: &str) -> bool {
        self.core().delivery_mut().unsubscribe(id)
    }
    pub fn visible(&self, visible: bool) -> View {
        let mut c = self.core();
        if c.state.apply_visibility(visible) {
            c.state.revision += 1;
        }
        c.refresh_control();
        if !c.allowed() {
            c.delivery_mut().clear_pending();
        }
        c.cached.clone()
    }
    pub fn native_visibility(&self, visible: bool) {
        let mut c = self.core();
        if c.native_visible != visible {
            c.native_visible = visible;
            c.state.revision += 1;
            c.refresh_control();
        }
        if !c.allowed() {
            c.delivery_mut().clear_pending();
        }
    }
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut c = self.core();
        let (flight, pending) = c.delivery_mut().counts();
        (flight, pending, self.controls.depth())
    }
    /// Internal bound probe for invariant tests: (live frames, frozen frames).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn retained_history_frames(&self) -> (usize, usize) {
        let c = self.core();
        (
            c.history.len(),
            c.frozen.as_ref().map_or(0, |frozen| frozen.history.len()),
        )
    }
    /// Atomically applies a complete selection to the active live or frozen source.
    pub fn set_view(&self, selection: ViewSelection) -> Result<View, CommandError> {
        let mut c = self.core();
        if c.closed {
            return Err(not_ready());
        }
        let supported = c
            .active_view()
            .process_summaries
            .as_ref()
            .map_or_else(Vec::new, |summaries| {
                summaries.supported_ranking_metrics.clone()
            });
        if !supported.contains(&selection.ranking_metric) {
            return Err(CommandError::new(
                CommandErrorCode::UnsupportedRanking,
                "The selected ranking is unavailable for the active source",
            ));
        }
        let mut state = c.state.clone();
        if !state.apply_selection(selection) {
            return Ok(c.cached.clone());
        }
        state.revision = state.revision.saturating_add(1);
        c.commit_selection(state, true)
    }
    /// Pause captures one frozen source; Resume discards it and renders current live data.
    pub fn set_paused(&self, paused: bool) -> Result<View, CommandError> {
        let mut c = self.core();
        if c.closed {
            return Err(not_ready());
        }
        if !c.state.apply_paused(paused) {
            return Ok(c.cached.clone());
        }
        if paused {
            let Some(id) = c.state.inspection_id.clone() else {
                return Err(not_ready());
            };
            let Some(frozen) = FrozenSource::capture(id, &c.live, &c.history) else {
                c.state.apply_paused(false);
                return Err(not_ready());
            };
            c.frozen = Some(frozen);
        } else {
            c.frozen = None;
        }
        c.state.revision = c.state.revision.saturating_add(1);
        let state = c.state.clone();
        c.commit_selection(state, true)
    }
    pub fn reconcile_gpus(&self, candidates: Vec<GpuCandidate>) {
        let mut c = self.core();
        if c.closed {
            return;
        }
        let mut candidates = candidates;
        candidates.retain(|g| g.descriptor.available);
        candidates.sort_by(|a, b| a.descriptor.id.cmp(&b.descriptor.id));
        candidates.dedup_by(|a, b| a.descriptor.id == b.descriptor.id);
        // Keep the chosen candidate within the bounded descriptor catalog, including saved choices.
        let saved = c
            .initial_gpu_id
            .as_ref()
            .or(c.live.preferences.selected_gpu_id.as_ref())
            .cloned();
        let chosen = select_gpu(&candidates, saved.as_deref()).map(|g| g.descriptor.clone());
        let mut gpus: Vec<_> = candidates
            .into_iter()
            .map(|g| g.descriptor)
            .take(MAX_GPUS)
            .collect();
        if let Some(chosen) = &chosen
            && !gpus.iter().any(|g| g.id == chosen.id)
        {
            gpus.pop();
            gpus.push(chosen.clone());
        }
        c.gpus = gpus;
        if c.state.mode == Mode::Paused {
            // A frozen inspection is never rebound to another device; Resume and the next
            // discovery reconcile the live selection instead.
            return;
        }
        c.initial_gpu_id = None;
        let selected = chosen.as_ref().map(|g| g.id.clone());
        let changed = selected != c.live.preferences.selected_gpu_id;
        if saved.is_some() && saved != selected {
            c.set_session_notice(Notice {
                code: "gpu_fallback".into(),
                message:
                    "The previously selected GPU is unavailable; using the next available adapter"
                        .into(),
            });
        }
        if changed {
            c.rebind_gpu(chosen);
        }
    }

    pub fn publish(
        &self,
        stamp: &PollStamp,
        elapsed: u64,
        wall: i64,
        host: HostIdentity,
        mut polls: Vec<ProviderPoll>,
    ) -> Result<(), CommandError> {
        self.publish_buffer(stamp, elapsed, wall, host, &mut polls)
    }
    fn publish_buffer(
        &self,
        stamp: &PollStamp,
        elapsed: u64,
        wall: i64,
        mut host: HostIdentity,
        polls: &mut [ProviderPoll],
    ) -> Result<(), CommandError> {
        let mut c = self.core();
        if c.closed || self.stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        let gpu_current =
            stamp.gpu_id == c.cached.preferences.selected_gpu_id && stamp.gpu_epoch == c.gpu_epoch;
        let power_current =
            stamp.power_source == c.power_source && stamp.power_epoch == c.power_epoch;
        if !gpu_current {
            for poll in polls.iter_mut() {
                poll.gpu = None;
                poll.process_gpu.clear();
                for p in &mut poll.processes {
                    p.gpu_pct = Reading {
                        scope_id: None,
                        ..Reading::unavailable("gpu:unavailable", "Awaiting the selected GPU")
                    };
                }
            }
        }
        host.session_id = c.cached.session_id.clone();
        let expected_gpu = c
            .cached
            .preferences
            .selected_gpu_id
            .as_ref()
            .map(|id| format!("gpu:{id}"));
        c.register_scopes(polls, expected_gpu.as_deref());
        let sequence = c.live.sample.as_ref().map_or(1, |s| s.sequence + 1);
        let template = CurrentSample {
            sequence,
            captured_at_ms: wall,
            elapsed_ms: elapsed,
            interval_ms: stamp.interval_ms,
            host,
            scopes: c.scopes.all(),
            capabilities: vec![],
            cpu: normalize::empty_cpu("cpu:system"),
            memory: normalize::empty_memory("host:system"),
            gpu: normalize::empty_gpu(expected_gpu.as_deref().unwrap_or("gpu:unavailable")),
            disk: normalize::empty_disk("volume:unavailable"),
            network: normalize::empty_network(providers::network::NETWORK_SCOPE),
            power: normalize::empty_power("power:primary"),
        };
        let mut sample = c.normalizer.normalize(template, polls);
        if expected_gpu.is_none() {
            macro_rules! absent { ($($field:ident),+) => {$(sample.gpu.$field = Reading { scope_id: None, ..Reading::unsupported("gpu:unavailable", "No GPU was discovered") };)+}; }
            absent!(
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
        }
        clean_scopes(&mut sample);
        if !power_current && sample.power.primary_kind == PowerPrimaryKind::GpuWatts {
            sample.power.watts = Reading::unavailable(
                "power:primary",
                "Awaiting a matching power-source observation",
            );
            sample.power.primary_value = sample.power.watts.clone();
        }
        c.bind_power_scope(&mut sample);
        clean_scopes(&mut sample);
        sample.capabilities = capabilities(&sample);
        sample.validate().map_err(|_| {
            CommandError::new(
                CommandErrorCode::Internal,
                "The collected readings could not be validated",
            )
        })?;
        let discontinuity = c.live.sample.as_ref().is_some_and(|old| {
            elapsed.saturating_sub(old.elapsed_ms) > stamp.interval_ms.saturating_mul(2)
        });
        let mut processes = Vec::new();
        for poll in polls.iter_mut() {
            if processes.is_empty() {
                processes = std::mem::take(&mut poll.processes);
            } else {
                processes.append(&mut poll.processes);
            }
        }
        for attribution in polls.iter().flat_map(|poll| poll.process_gpu.iter()) {
            if let Some(process) = processes
                .iter_mut()
                .find(|process| process.pid == attribution.pid)
            {
                process.gpu_pct = attribution.reading.clone();
            }
        }
        for process in &mut processes {
            clean_process_scopes(process, &sample.scopes);
        }
        let warmed = c
            .history
            .frames()
            .front()
            .is_some_and(|first| elapsed.saturating_sub(first.elapsed_ms) >= stamp.interval_ms);
        let mut notices = Vec::with_capacity(bounds::MAX_NOTICES);
        notices.extend(c.session_notices.iter().cloned());
        notices.extend(baseline::evaluate(&sample, &processes, warmed).notice);
        if !warmed {
            notices.push(Notice {
                code: "initializing".into(),
                message: "Initializing delta counters".into(),
            });
        }
        for metadata in polls.iter().filter_map(|p| p.network_members.as_ref()) {
            if metadata.total_member_count == 0
                && sample.network.receive_bytes_per_second.status == ReadingStatus::Valid
            {
                notices.push(Notice {
                    code: "network_empty".into(),
                    message: "No active interfaces".into(),
                });
            }
            if metadata.omitted_member_count > 0 {
                notices.push(Notice{code:"network_members_omitted".into(),message:format!("Interface traffic includes all {} interfaces; {} descriptive member IDs omitted",metadata.total_member_count,metadata.omitted_member_count)});
            }
        }
        c.history
            .append(ScalarFrame::from_sample(&sample, discontinuity));
        let live_state = c.live_state();
        let next = view::build(
            &live_state,
            sample,
            &c.history,
            &processes,
            c.live.preferences.clone(),
            notices,
        );
        next.validate().map_err(|_| {
            CommandError::new(
                CommandErrorCode::Internal,
                "The current view could not be validated",
            )
        })?;
        c.live = next.clone();
        if c.state.mode == Mode::Paused {
            // The frozen inspection keeps rendering while live collection advances privately.
            c.delivery_mut().clear_pending();
            return Ok(());
        }
        c.cached = next;
        c.refresh_control();
        let latest = c.cached.clone();
        let allowed = c.allowed();
        c.delivery_mut().offer(latest, allowed)
    }
    pub fn start(self: &Arc<Self>) {
        let mut handle = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if handle.is_some() {
            return;
        }
        let weak = Arc::downgrade(self);
        let stopping = self.stopping.clone();
        let wake = self.wake.clone();
        let started = std::thread::Builder::new()
            .name("telemetry-collector".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                else {
                    if let Some(service) = weak.upgrade() {
                        service.collection_error("The collection timer could not start");
                    }
                    return;
                };
                runtime.block_on(async move {
                    let clock = SessionClock::new(SystemClock);
                    let mut native = NativeProviders::new();
                    let mut polls = Vec::with_capacity(8);
                    let mut host = host_identity();
                    let mut last_discovery = None;
                    let mut loss = false;
                    let mut ticker = tokio::time::interval(Duration::from_millis(1000));
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut configured_interval = 1000u64;
                    loop {
                        let Some(service) = weak.upgrade() else {
                            break;
                        };
                        let desired = service.capture_poll().interval_ms;
                        if desired != configured_interval {
                            configured_interval = desired;
                            ticker = tokio::time::interval_at(
                                tokio::time::Instant::now()
                                    + Duration::from_millis(configured_interval),
                                Duration::from_millis(configured_interval),
                            );
                            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        }
                        tokio::select! {_=ticker.tick()=>{},_=wake.notified()=>{}}
                        if stopping.load(Ordering::Acquire) {
                            break;
                        }
                        let elapsed = clock.elapsed_ms();
                        if last_discovery.is_none_or(|last| elapsed.saturating_sub(last) >= 30_000)
                            || loss
                        {
                            service.reconcile_gpus(native.discover());
                            last_discovery = Some(elapsed);
                        }
                        let stamp = service.capture_poll();
                        native.select(stamp.gpu_id.as_deref());
                        let ctx = PollContext {
                            clock: clock.source(),
                            elapsed_ms: clock.elapsed_ms(),
                            interval_ms: stamp.interval_ms,
                        };
                        native.poll(&ctx, &mut polls);
                        loss = polls.iter().any(|p| {
                            !p.losses.is_empty()
                                || p.gpu.as_ref().is_some_and(|g| {
                                    g.utilization_pct.status == ReadingStatus::Unavailable
                                        && g.vram_total_bytes.status == ReadingStatus::Unavailable
                                })
                        });
                        host.uptime_seconds = native_uptime();
                        if let Err(error) = service.publish_buffer(
                            &stamp,
                            clock.elapsed_ms(),
                            clock.wall_ms(),
                            host.clone(),
                            &mut polls,
                        ) {
                            let mut c = service.core();
                            c.cached.notices.retain(|n| n.code != "collection_error");
                            c.cached.notices.push(Notice {
                                code: "collection_error".into(),
                                message: error.message,
                            });
                            c.cached.notices.truncate(bounds::MAX_NOTICES);
                        }
                    }
                    providers::power::release_battery_owner();
                });
            });
        match started {
            Ok(worker) => *handle = Some(worker),
            Err(_) => self.collection_error("The collection worker could not start"),
        }
    }
    fn collection_error(&self, message: &str) {
        let mut c = self.core();
        c.cached.notices.retain(|n| n.code != "collection_error");
        c.cached.notices.push(Notice {
            code: "collection_error".into(),
            message: bounds::bounded_message(message),
        });
        c.cached.notices.truncate(bounds::MAX_NOTICES);
    }
    pub fn close(&self) {
        self.stopping.store(true, Ordering::Release);
        self.wake.notify_one();
        let mut c = self.core();
        c.closed = true;
        c.delivery_mut().clear();
        c.history.clear();
        c.frozen = None;
        c.normalizer.reset();
        c.session_notices.clear();
        c.gpus.clear();
        c.live = view::initializing(c.cached.session_id.clone());
        c.cached = c.live.clone();
    }
    #[cfg(feature = "test-fixtures")]
    pub fn install_probe(self: &Arc<Self>, probe: &Arc<providers::fixtures::LiveProbe>) {
        let mut c = self.core();
        if c.fixture_installed {
            return;
        }
        let Ok(json) =
            serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/contract-v1.json"))
        else {
            return;
        };
        let Ok(bootstrap) = serde_json::from_value::<Bootstrap>(json["bootstrap"].clone()) else {
            return;
        };
        c.state.revision = bootstrap.view.revision;
        c.gpus = bootstrap.gpus;
        c.cached = bootstrap.view;
        c.fixture_installed = true;
        let weak = Arc::downgrade(self);
        let counts_weak = weak.clone();
        probe.install(providers::fixtures::LiveHooks {
            publish: Box::new(move |mut view| {
                if let Some(s) = weak.upgrade() {
                    let mut c = s.core();
                    view.revision = c.state.revision;
                    view.visible = c.state.visible && c.native_visible;
                    if view.validate().is_err() {
                        return;
                    }
                    c.live = view.clone();
                    if c.state.mode == Mode::Paused {
                        return;
                    }
                    c.cached = view.clone();
                    let allowed = c.allowed();
                    let _ = c.delivery_mut().offer(view, allowed);
                }
            }),
            counts: Box::new(move || {
                let (in_flight, pending, queued_controls) =
                    counts_weak.upgrade().map_or((0, 0, 0), |s| s.counts());
                providers::fixtures::LiveCounts {
                    in_flight,
                    pending,
                    queued_controls,
                }
            }),
        });
    }
}
impl Drop for TelemetryService {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.wake.notify_one();
    }
}
pub fn not_ready() -> CommandError {
    CommandError::new(
        CommandErrorCode::NotReady,
        "This action is not currently available",
    )
}
impl Core {
    fn set_session_notice(&mut self, notice: Notice) {
        self.session_notices.retain(|old| old.code != notice.code);
        self.session_notices.push(notice);
        self.refresh_session_notices();
    }
    fn refresh_session_notices(&mut self) {
        sync_session_notices(&mut self.cached, &self.session_notices);
        sync_session_notices(&mut self.live, &self.session_notices);
    }
    fn rebind_gpu(&mut self, chosen: Option<GpuDescriptor>) {
        let selected = chosen.as_ref().map(|g| g.id.clone());
        self.gpu_epoch += 1;
        let gpu_power = self
            .cached
            .sample
            .as_ref()
            .is_some_and(|sample| sample.power.primary_kind == PowerPrimaryKind::GpuWatts);
        if gpu_power {
            self.power_epoch += 1;
            let epoch = self.power_epoch;
            self.scopes.set_epoch("power:primary", epoch);
        }
        self.state.revision += 1;
        self.cached.preferences.selected_gpu_id = selected;
        self.live.preferences.selected_gpu_id = self.cached.preferences.selected_gpu_id.clone();
        if let Some(mut sample) = self.cached.sample.clone() {
            let old: Vec<_> = sample
                .scopes
                .iter()
                .filter(|scope| scope.kind == ScopeKind::Gpu && scope.id != "power:primary")
                .map(|scope| scope.id.clone())
                .collect();
            for id in old {
                self.scopes.remove(&id);
                sample.scopes.retain(|scope| scope.id != id);
            }
            let scope = chosen.as_ref().map(|gpu| format!("gpu:{}", gpu.id));
            sample.gpu = normalize::empty_gpu(scope.as_deref().unwrap_or("gpu:unavailable"));
            if let Some(gpu) = &chosen {
                sample.scopes.push(ResourceScope {
                    id: format!("gpu:{}", gpu.id),
                    kind: ScopeKind::Gpu,
                    label: gpu.name.clone(),
                    provider: "native".into(),
                    epoch: self.gpu_epoch,
                });
            }
            sample.power.gpu_board_w = sample.gpu.power_w.clone();
            sample.power.gpu_temperature_c = sample.gpu.temperature_c.clone();
            sample.power.fan_pct = sample.gpu.fan_pct.clone();
            sample.power.power_limit_pct = sample.gpu.power_limit_pct.clone();
            if sample.power.primary_kind == PowerPrimaryKind::GpuWatts {
                sample.power.watts =
                    Reading::unavailable("power:primary", "Awaiting the selected GPU");
                sample.power.primary_value = sample.power.watts.clone();
                if let Some(scope) = sample.scopes.iter_mut().find(|s| s.id == "power:primary") {
                    scope.epoch = self.power_epoch;
                }
            }
            clean_scopes(&mut sample);
            sample.capabilities = capabilities(&sample);
            self.cached.sample = Some(sample.clone());
            if let Some(summaries) = self.cached.process_summaries.as_mut() {
                summaries.gpu_top.clear();
                summaries
                    .supported_ranking_metrics
                    .retain(|metric| *metric != RankingMetric::Gpu);
                for process in summaries
                    .memory_top
                    .iter_mut()
                    .chain(summaries.cpu_top.iter_mut())
                    .chain(summaries.power_top.iter_mut())
                {
                    process.gpu_pct = Reading {
                        scope_id: scope.clone(),
                        ..Reading::unavailable("gpu:unavailable", "Awaiting the selected GPU")
                    };
                    clean_process_scopes(process, &sample.scopes);
                }
            }
            self.cached.memory_breakdowns.vram = None;
            self.cached.history = Some(crate::telemetry::projection::project(
                &self.history,
                sample.elapsed_ms,
                self.state.range,
                &sample,
            ));
        }
        self.refresh_control();
        self.delivery_mut().clear_pending();
        // The private live source must carry the same rebinding before the next poll.
        self.live = self.cached.clone();
    }
    fn delivery_mut(&mut self) -> &mut DeliveryState {
        &mut self.delivery
    }
    fn allowed(&self) -> bool {
        !self.closed && self.state.visible && self.native_visible && self.state.mode == Mode::Live
    }
    fn active_view(&self) -> &View {
        self.frozen
            .as_ref()
            .map_or(&self.live, |frozen| &frozen.view)
    }
    /// Live collection always builds a live-mode view even while a frozen source is displayed.
    fn live_state(&self) -> ViewState {
        let mut state = self.state.clone();
        state.mode = Mode::Live;
        state.inspection_id = None;
        state
    }
    /// Projects the active source through one history ring and commits a coherent control result.
    fn commit_selection(
        &mut self,
        state: ViewState,
        clear_pending: bool,
    ) -> Result<View, CommandError> {
        let visible = state.visible && self.native_visible;
        let preferences = self.cached.preferences.clone();
        let next = match self.frozen.as_ref() {
            Some(frozen) => {
                view::reselect(&state, &frozen.view, &frozen.history, preferences, visible)
            }
            None => view::reselect(&state, &self.live, &self.history, preferences, visible),
        };
        if let Some(mut next) = next {
            // Configuration follows current collection preferences; telemetry/baseline stays frozen.
            sync_session_notices(&mut next, &self.session_notices);
            next.validate().map_err(|_| {
                CommandError::new(
                    CommandErrorCode::Internal,
                    "The current view could not be validated",
                )
            })?;
            self.state = state;
            self.cached = next;
        } else {
            self.state = state;
            self.refresh_control();
        }
        if clear_pending {
            self.delivery_mut().clear_pending();
        }
        Ok(self.cached.clone())
    }
    fn refresh_control(&mut self) {
        self.cached.revision = self.state.revision;
        self.cached.visible = self.state.visible && self.native_visible;
    }
    fn register_scopes(&mut self, polls: &[ProviderPoll], gpu_scope: Option<&str>) {
        let mut seen = BTreeSet::new();
        let mut register = |id: &str, kind: ScopeKind, label: &str, provider: &str| {
            if id.is_empty()
                || id.len() > 128
                || id.ends_with("unavailable")
                || id == "gpu:unselected"
                || seen.len() >= MAX_SCOPES - 1
            {
                return;
            }
            self.scopes
                .upsert(id, kind, bounds::bounded_label(label, 128), provider);
            seen.insert(id.to_owned());
        };
        register("host:system", ScopeKind::Host, "System RAM", "sysinfo");
        register("cpu:system", ScopeKind::CpuPackage, "CPU", "native");
        macro_rules! fields {
            ($group:expr, $kind:expr, $label:expr, $provider:expr, $($field:ident),+) => {$(
                if let Some(id) = $group.$field.scope_id.as_deref() { register(id, $kind, $label, $provider); }
            )+};
        }
        for poll in polls {
            if let Some(group) = &poll.cpu {
                fields!(
                    group,
                    ScopeKind::CpuPackage,
                    "CPU",
                    "native",
                    total_pct,
                    user_pct,
                    system_pct,
                    temperature_c
                );
            }
            if let Some(group) = &poll.memory {
                fields!(
                    group,
                    ScopeKind::Host,
                    "System RAM",
                    "sysinfo",
                    total_bytes,
                    used_bytes,
                    available_bytes,
                    cached_bytes,
                    used_pct
                );
            }
            if let Some(group) = &poll.disk {
                fields!(
                    group,
                    ScopeKind::Volume,
                    if cfg!(target_os = "linux") {
                        "System volume (/)"
                    } else {
                        "System volume"
                    },
                    "native",
                    total_bytes,
                    free_bytes,
                    read_bytes_per_second,
                    write_bytes_per_second,
                    written_session_bytes
                );
            }
            if let Some(group) = &poll.network {
                fields!(
                    group,
                    ScopeKind::NetworkAggregate,
                    "Interface traffic",
                    "native",
                    receive_bytes_per_second,
                    send_bytes_per_second,
                    received_session_bytes,
                    sent_session_bytes
                );
            }
            if let Some(group) = &poll.power {
                macro_rules! power_scope { ($($field:ident),+) => {$(
                    if let Some(id) = group.$field.scope_id.as_deref() {
                        if id.starts_with("battery:") { register(id, ScopeKind::Battery, "Battery", "battery"); }
                        else if id.starts_with("cpu:") { register(id, ScopeKind::CpuPackage, "CPU package (RAPL)", "native"); }
                    }
                )+}; }
                power_scope!(
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
        }
        if let Some(id) = gpu_scope {
            let label = self
                .gpus
                .iter()
                .find(|g| format!("gpu:{}", g.id) == id)
                .map_or("Selected GPU", |g| g.name.as_str());
            register(id, ScopeKind::Gpu, label, "native");
        }
        if self.scopes.get("power:primary").is_some() {
            seen.insert("power:primary".into());
        }
        self.scopes.reconcile(&seen.into_iter().collect::<Vec<_>>());
        if let Some(id) = gpu_scope {
            self.scopes.set_epoch(id, self.gpu_epoch);
        }
    }
    fn bind_power_scope(&mut self, sample: &mut CurrentSample) {
        let source = sample
            .power
            .watts
            .scope_id
            .clone()
            .filter(|id| id != "power:primary" && sample.scopes.iter().any(|s| &s.id == id));
        if source != self.power_source {
            self.power_epoch += 1;
            self.state.revision += 1;
            self.power_source = source.clone();
        }
        if let Some(source) = source
            && let Some(scope) = sample.scopes.iter().find(|s| s.id == source).cloned()
        {
            let label = match scope.kind {
                ScopeKind::Battery => "Battery discharge",
                ScopeKind::Gpu => "GPU board",
                _ => "CPU package",
            };
            self.scopes
                .upsert("power:primary", scope.kind, label, scope.provider);
            self.scopes.set_epoch("power:primary", self.power_epoch);
            sample.power.watts.scope_id = Some("power:primary".into());
            if matches!(
                sample.power.primary_kind,
                PowerPrimaryKind::GpuWatts | PowerPrimaryKind::CpuPackageWatts
            ) {
                sample.power.primary_value.scope_id = Some("power:primary".into());
            }
        } else {
            self.scopes.remove("power:primary");
        }
        sample.scopes = self.scopes.all();
    }
}
fn sync_session_notices(view: &mut View, notices: &[Notice]) {
    view.notices.retain(|notice| {
        !matches!(
            notice.code.as_str(),
            "preferences_reset" | "gpu_fallback" | "preferences_not_saved"
        )
    });
    view.notices.extend(notices.iter().cloned());
    view.notices.truncate(bounds::MAX_NOTICES);
}
macro_rules! each_reading {
    ($s:ident, $apply:ident) => {
        $apply!($s.cpu.total_pct, "cpu.totalPct", MetricUnit::Pct);
        $apply!($s.cpu.user_pct, "cpu.userPct", MetricUnit::Pct);
        $apply!($s.cpu.system_pct, "cpu.systemPct", MetricUnit::Pct);
        $apply!(
            $s.cpu.temperature_c,
            "cpu.temperatureC",
            MetricUnit::Celsius
        );
        $apply!(
            $s.memory.total_bytes,
            "memory.totalBytes",
            MetricUnit::Bytes
        );
        $apply!($s.memory.used_bytes, "memory.usedBytes", MetricUnit::Bytes);
        $apply!(
            $s.memory.available_bytes,
            "memory.availableBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.memory.cached_bytes,
            "memory.cachedBytes",
            MetricUnit::Bytes
        );
        $apply!($s.memory.used_pct, "memory.usedPct", MetricUnit::Pct);
        $apply!(
            $s.gpu.utilization_pct,
            "gpu.utilizationPct",
            MetricUnit::Pct
        );
        $apply!(
            $s.gpu.vram_total_bytes,
            "gpu.vramTotalBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.gpu.vram_used_bytes,
            "gpu.vramUsedBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.gpu.vram_free_bytes,
            "gpu.vramFreeBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.gpu.vram_reserved_bytes,
            "gpu.vramReservedBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.gpu.temperature_c,
            "gpu.temperatureC",
            MetricUnit::Celsius
        );
        $apply!($s.gpu.power_w, "gpu.powerW", MetricUnit::Watts);
        $apply!($s.gpu.fan_pct, "gpu.fanPct", MetricUnit::Pct);
        $apply!($s.gpu.power_limit_pct, "gpu.powerLimitPct", MetricUnit::Pct);
        $apply!($s.disk.total_bytes, "disk.totalBytes", MetricUnit::Bytes);
        $apply!($s.disk.free_bytes, "disk.freeBytes", MetricUnit::Bytes);
        $apply!(
            $s.disk.read_bytes_per_second,
            "disk.readBytesPerSecond",
            MetricUnit::BytesPerSecond
        );
        $apply!(
            $s.disk.write_bytes_per_second,
            "disk.writeBytesPerSecond",
            MetricUnit::BytesPerSecond
        );
        $apply!(
            $s.disk.written_session_bytes,
            "disk.writtenSessionBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.network.receive_bytes_per_second,
            "network.receiveBytesPerSecond",
            MetricUnit::BytesPerSecond
        );
        $apply!(
            $s.network.send_bytes_per_second,
            "network.sendBytesPerSecond",
            MetricUnit::BytesPerSecond
        );
        $apply!(
            $s.network.received_session_bytes,
            "network.receivedSessionBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.network.sent_session_bytes,
            "network.sentSessionBytes",
            MetricUnit::Bytes
        );
        $apply!(
            $s.power.primary_value,
            "power.primaryValue",
            if $s.power.primary_kind == PowerPrimaryKind::BatteryPct {
                MetricUnit::Pct
            } else {
                MetricUnit::Watts
            }
        );
        $apply!($s.power.watts, "power.watts", MetricUnit::Watts);
        $apply!(
            $s.power.cpu_package_w,
            "power.cpuPackageW",
            MetricUnit::Watts
        );
        $apply!($s.power.gpu_board_w, "power.gpuBoardW", MetricUnit::Watts);
        $apply!(
            $s.power.gpu_temperature_c,
            "power.gpuTemperatureC",
            MetricUnit::Celsius
        );
        $apply!($s.power.fan_pct, "power.fanPct", MetricUnit::Pct);
        $apply!(
            $s.power.power_limit_pct,
            "power.powerLimitPct",
            MetricUnit::Pct
        );
        $apply!(
            $s.power.battery_charge_pct,
            "power.batteryChargePct",
            MetricUnit::Pct
        );
        $apply!(
            $s.power.battery_health_pct,
            "power.batteryHealthPct",
            MetricUnit::Pct
        );
        $apply!(
            $s.power.battery_remaining_seconds,
            "power.batteryRemainingSeconds",
            MetricUnit::Seconds
        );
        $apply!(
            $s.power.battery_state,
            "power.batteryState",
            MetricUnit::State
        );
    };
}
fn clean_reading<T>(reading: &mut Reading<T>, scopes: &[ResourceScope]) {
    if reading
        .scope_id
        .as_ref()
        .is_some_and(|id| scopes.iter().any(|s| &s.id == id))
    {
        return;
    }
    reading.scope_id = None;
    if matches!(reading.status, ReadingStatus::Valid | ReadingStatus::Stale) {
        reading.status = ReadingStatus::Unavailable;
        reading.value = None;
        reading.sampled_at_ms = None;
        reading.elapsed_ms = None;
        reading.reason = Some("No matching resource scope was discovered".into());
    }
}
fn clean_scopes(s: &mut CurrentSample) {
    let scopes = &s.scopes;
    macro_rules! clean {
        ($reading:expr, $id:expr, $unit:expr) => {
            clean_reading(&mut $reading, scopes);
        };
    }
    each_reading!(s, clean);
}
fn clean_process_scopes(process: &mut ProcessObservation, scopes: &[ResourceScope]) {
    clean_reading(&mut process.resident_bytes, scopes);
    clean_reading(&mut process.cpu_pct, scopes);
    clean_reading(&mut process.gpu_pct, scopes);
    clean_reading(&mut process.power_w, scopes);
}
fn capability<T>(
    reading: &Reading<T>,
    metric: &str,
    unit: MetricUnit,
    scopes: &[ResourceScope],
) -> Option<Capability> {
    let id = reading.scope_id.as_ref()?;
    let scope = scopes.iter().find(|s| &s.id == id)?;
    let status = if reading.status == ReadingStatus::Stale {
        ReadingStatus::Unavailable
    } else {
        reading.status
    };
    Some(Capability {
        metric_id: metric.into(),
        scope_id: id.clone(),
        unit,
        provider: scope.provider.clone(),
        status,
        reason: if status == ReadingStatus::Valid {
            None
        } else {
            Some(bounds::bounded_message(
                reading
                    .reason
                    .as_deref()
                    .unwrap_or("No current observation is available"),
            ))
        },
    })
}
fn capabilities(s: &CurrentSample) -> Vec<Capability> {
    let mut caps = Vec::with_capacity(METRIC_CATALOG.len());
    macro_rules! cap {
        ($reading:expr, $id:expr, $unit:expr) => {
            if let Some(value) = capability(&$reading, $id, $unit, &s.scopes) {
                caps.push(value);
            }
        };
    }
    each_reading!(s, cap);
    caps
}
fn host_identity() -> HostIdentity {
    let mut system = sysinfo::System::new();
    system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    #[cfg(target_os = "linux")]
    let guest = {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .to_lowercase();
        kernel.contains("microsoft-standard")
            || kernel.contains("wsl2")
            || std::env::var_os("WSL_INTEROP").is_some()
    };
    #[cfg(not(target_os = "linux"))]
    let guest = false;
    let label = |s: String| bounds::bounded_label(&s, 128);
    HostIdentity {
        session_id: uuid::Uuid::new_v4().to_string(),
        hostname: label(
            sysinfo::System::host_name().unwrap_or_else(|| "Hostname unavailable".into()),
        ),
        os_name: label(
            sysinfo::System::name().unwrap_or_else(|| "Operating system unavailable".into()),
        ),
        os_version: label(
            sysinfo::System::os_version().unwrap_or_else(|| "Version unavailable".into()),
        ),
        cpu_model: label(
            system
                .cpus()
                .first()
                .map_or("CPU model unavailable", |c| c.brand())
                .to_string(),
        ),
        logical_cpu_count: (system.cpus().len() as u32).max(1),
        environment: if guest {
            Environment::Wsl2
        } else {
            Environment::Native
        },
        uptime_seconds: native_uptime(),
        scope_label: if guest {
            "WSL2 guest".into()
        } else {
            "Native host".into()
        },
    }
}
fn native_uptime() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/uptime")
            .ok()?
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| v as u64)
    }
    #[cfg(target_os = "windows")]
    {
        Some(unsafe { ::windows::Win32::System::SystemInformation::GetTickCount64() } / 1000)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
struct NativeProviders {
    base: Vec<Box<dyn Provider>>,
    nvml: Option<providers::nvml::NvmlProvider>,
    selected: Option<String>,
    fallback: Option<Box<dyn Provider>>,
    #[cfg(target_os = "linux")]
    drm: Vec<providers::linux::gpu::DrmAdapter>,
}
impl NativeProviders {
    fn new() -> Self {
        let mut base: Vec<Box<dyn Provider>> = vec![
            Box::new(providers::system::SystemProvider::new()),
            Box::<providers::network::NetworkProvider>::default(),
            Box::new(providers::power::BatteryProvider),
        ];
        #[cfg(target_os = "linux")]
        {
            base.push(Box::<providers::linux::system::LinuxSystemProvider>::default());
            base.push(Box::<providers::linux::power::RaplProvider>::default());
        }
        #[cfg(target_os = "windows")]
        {
            base.push(Box::<providers::windows::system::WindowsSystemProvider>::default());
        }
        Self {
            base,
            nvml: providers::nvml::NvmlProvider::new(String::new()).ok(),
            selected: None,
            fallback: None,
            #[cfg(target_os = "linux")]
            drm: vec![],
        }
    }
    fn discover(&mut self) -> Vec<GpuCandidate> {
        if self.nvml.is_none() {
            self.nvml = providers::nvml::NvmlProvider::new(String::new()).ok();
        }
        let mut gpus: Vec<_> = self
            .nvml
            .as_ref()
            .map_or(vec![], |nv| nv.discover())
            .into_iter()
            .map(|descriptor| GpuCandidate {
                descriptor,
                identity_kind: GpuIdentityKind::NvmlUuid,
            })
            .collect();
        #[cfg(target_os = "linux")]
        {
            self.drm = providers::linux::gpu::discover().unwrap_or_default();
            for adapter in &self.drm {
                let id = format!("pci:{}", adapter.pci_id);
                let name = format!(
                    "{} GPU ({})",
                    match adapter.vendor {
                        GpuVendor::Amd => "AMD",
                        GpuVendor::Intel => "Intel",
                        _ => "DRM",
                    },
                    adapter.pci_id
                );
                gpus.push(GpuCandidate::new(
                    id,
                    name,
                    adapter.vendor,
                    adapter.kind,
                    true,
                    GpuIdentityKind::LinuxPci,
                ));
            }
        }
        #[cfg(target_os = "windows")]
        {
            for adapter in providers::windows::gpu::discover().unwrap_or_default() {
                if adapter.descriptor.vendor != GpuVendor::Nvidia || gpus.is_empty() {
                    gpus.push(GpuCandidate {
                        descriptor: adapter.descriptor,
                        identity_kind: GpuIdentityKind::WindowsPnp,
                    });
                }
            }
        }
        gpus
    }
    fn select(&mut self, id: Option<&str>) {
        if self.selected.as_deref() == id {
            return;
        }
        self.selected = id.map(str::to_owned);
        self.fallback = None;
        if let Some(id) = id {
            if id.starts_with("GPU-")
                && let Some(nvml) = self.nvml.as_mut()
            {
                nvml.select(id.into());
            }
            #[cfg(target_os = "linux")]
            if let Some(pci) = id.strip_prefix("pci:")
                && let Some(adapter) = self.drm.iter().find(|a| a.pci_id == pci)
            {
                self.fallback = Some(Box::new(providers::linux::gpu::DrmProvider::new(
                    adapter.clone(),
                )));
            }
            #[cfg(target_os = "windows")]
            if !id.starts_with("GPU-") {
                self.fallback = Some(Box::new(providers::windows::gpu::WindowsGpuProvider::new(
                    id.into(),
                )));
            }
        }
    }
    fn poll(&self, ctx: &PollContext<'_>, polls: &mut Vec<ProviderPoll>) {
        polls.clear();
        let start = ctx.clock.boot_ms();
        let poll = |provider: &dyn Provider| {
            let current = PollContext {
                clock: ctx.clock,
                elapsed_ms: ctx
                    .elapsed_ms
                    .saturating_add(ctx.clock.boot_ms().saturating_sub(start)),
                interval_ms: ctx.interval_ms,
            };
            provider.poll(&current)
        };
        polls.extend(self.base.iter().map(|p| poll(p.as_ref())));
        if self
            .selected
            .as_ref()
            .is_some_and(|id| id.starts_with("GPU-"))
        {
            if let Some(nvml) = &self.nvml {
                polls.push(poll(nvml));
            }
        } else if let Some(provider) = &self.fallback {
            polls.push(poll(provider.as_ref()));
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
#[test]
fn windows_host_identity_is_native_even_when_launched_from_wsl() {
    assert_eq!(host_identity().environment, Environment::Native);
}
