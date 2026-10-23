//! Strict typed command envelopes for the bundled main webview. Sensor I/O never runs here.
use crate::telemetry::{
    collector::TelemetryService,
    model::*,
    state::{CommandError, CommandErrorCode, CommandResult},
};
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::Arc;
#[cfg(feature = "test-fixtures")]
use tauri::Manager;
use tauri::{
    Runtime, State, WebviewWindow,
    ipc::{InvokeBody, JavaScriptChannelId, Request},
};
fn invalid() -> CommandError {
    CommandError::new(
        CommandErrorCode::InvalidArgument,
        "The command arguments are invalid",
    )
}
fn input<T: DeserializeOwned>(request: &Request<'_>, required: &[&str]) -> Result<T, CommandError> {
    let InvokeBody::Json(json) = request.body() else {
        return Err(invalid());
    };
    let object = json.as_object().ok_or_else(invalid)?;
    if required.iter().any(|key| !object.contains_key(*key)) {
        return Err(invalid());
    }
    serde_json::from_value(json.clone()).map_err(|_| invalid())
}
fn owner<R: Runtime>(
    window: &WebviewWindow<R>,
    service: &Arc<TelemetryService>,
) -> Result<crate::telemetry::state::ControlPermit, CommandError> {
    if window.label() != "main" {
        return Err(invalid());
    }
    #[cfg(feature = "test-fixtures")]
    if let Some(probe) = window
        .app_handle()
        .try_state::<Arc<crate::telemetry::providers::fixtures::LiveProbe>>()
    {
        service.install_probe(&probe);
    }
    service.admit()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Subscribe {
    on_view: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack {
    subscription_id: String,
    delivery_sequence: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Unsubscribe {
    subscription_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Visibility {
    visible: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetView {
    selection: ViewSelection,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPaused {
    paused: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetPreferences {
    interval_ms: u64,
    selected_gpu_id: Option<String>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    subscription_id: String,
    session_id: String,
}
#[derive(serde::Serialize)]
pub struct Accepted {
    accepted: bool,
}
#[derive(serde::Serialize)]
pub struct Removed {
    removed: bool,
}
#[derive(serde::Serialize)]
pub struct ViewResponse {
    view: View,
}
#[derive(serde::Serialize)]
pub struct PreferencesResponse {
    view: View,
    persisted: bool,
}
#[tauri::command]
pub fn bootstrap<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<Bootstrap> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let _: Empty = input(&request, &[])?;
        Ok(service.bootstrap())
    })())
}
#[tauri::command]
pub fn subscribe_telemetry<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<Subscription> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: Subscribe = input(&request, &["onView"])?;
        let id: JavaScriptChannelId = args.on_view.parse().map_err(|_| invalid())?;
        let channel = id.channel_on::<R, Delivery>(window.as_ref().clone());
        let (subscription_id, session_id) = service.subscribe(Arc::new(move |delivery| {
            channel.send(delivery).map_err(|_| ())
        }))?;
        Ok(Subscription {
            subscription_id,
            session_id,
        })
    })())
}
#[tauri::command]
pub fn ack_telemetry<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<Accepted> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: Ack = input(&request, &["subscriptionId", "deliverySequence"])?;
        if !is_uuid(&args.subscription_id) || !is_json_safe(args.delivery_sequence) {
            return Err(invalid());
        }
        Ok(Accepted {
            accepted: service.ack(&args.subscription_id, args.delivery_sequence)?,
        })
    })())
}
#[tauri::command]
pub fn unsubscribe_telemetry<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<Removed> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: Unsubscribe = input(&request, &["subscriptionId"])?;
        if !is_uuid(&args.subscription_id) {
            return Err(invalid());
        }
        Ok(Removed {
            removed: service.unsubscribe(&args.subscription_id),
        })
    })())
}
#[tauri::command]
pub fn set_delivery_visibility<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<ViewResponse> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: Visibility = input(&request, &["visible"])?;
        if let (Ok(visible), Ok(minimized)) = (window.is_visible(), window.is_minimized()) {
            service.native_visibility(visible && !minimized);
        }
        Ok(ViewResponse {
            view: service.visible(args.visible),
        })
    })())
}
// Settings persistence and GPU selection belong to US4; set_preferences remains reserved.
#[tauri::command]
pub fn set_view<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<ViewResponse> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: SetView = input(&request, &["selection"])?;
        Ok(ViewResponse {
            view: service.set_view(args.selection)?,
        })
    })())
}
#[tauri::command]
pub fn set_paused<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<ViewResponse> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: SetPaused = input(&request, &["paused"])?;
        Ok(ViewResponse {
            view: service.set_paused(args.paused)?,
        })
    })())
}
#[tauri::command]
pub fn set_preferences<R: Runtime>(
    window: WebviewWindow<R>,
    service: State<'_, Arc<TelemetryService>>,
    request: Request<'_>,
) -> CommandResult<PreferencesResponse> {
    CommandResult::from_result((|| {
        let _permit = owner(&window, &service)?;
        let args: SetPreferences = input(&request, &["intervalMs", "selectedGpuId"])?;
        let (view, persisted) = service.set_preferences(args.interval_ms, args.selected_gpu_id)?;
        Ok(PreferencesResponse { view, persisted })
    })())
}
