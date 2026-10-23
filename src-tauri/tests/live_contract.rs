//! US1 commands and Channel exercised through the real handler and generated main-window ACL.
use meowndesk_lib::telemetry::{
    model::Bootstrap,
    state::{CommandErrorCode, ControlQueue},
};
use serde_json::{Value, json};

fn context() -> tauri::Context<tauri::test::MockRuntime> {
    // Exercise the real generated main-window ACL; an empty mock context rejects every custom
    // command before dispatch and would test the harness rather than the missing live behavior.
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context
}
fn invoke(command: &str, args: Value) -> Value {
    let app = meowndesk_lib::configure(tauri::test::mock_builder())
        .build(context())
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let response = tauri::test::get_ipc_response(
        &window,
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: window.url().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    );
    response
        .unwrap_or_else(|e| {
            panic!("{command} must be registered and return a typed envelope: {e:?}")
        })
        .deserialize()
        .unwrap()
}
#[test]
fn bootstrap_has_complete_cached_shape_and_visibility_returns_a_typed_view() {
    let result = invoke("bootstrap", json!({}));
    assert_eq!(result["ok"], true);
    let bootstrap: Bootstrap = serde_json::from_value(result["data"].clone()).unwrap();
    bootstrap.validate().unwrap();
    let result = invoke("set_delivery_visibility", json!({"visible":false}));
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["view"]["visible"], false);
}
#[test]
fn unowned_acks_and_unsubscribe_are_harmless_typed_results() {
    let id = "b407b2ac-718f-4c7c-a909-f8dd1e3bb1b0";
    for sequence in [0, 1, 1, 0, 99] {
        let result = invoke(
            "ack_telemetry",
            json!({"subscriptionId":id,"deliverySequence":sequence}),
        );
        assert!(
            result["data"]["accepted"] == false
                || result["error"]["code"] == "invalid_subscription"
        );
    }
    let result = invoke("unsubscribe_telemetry", json!({"subscriptionId":id}));
    assert_eq!(result["data"]["removed"], false);
}
#[test]
fn control_admission_is_bounded_at_32_and_released() {
    let queue = ControlQueue::default();
    let permits: Vec<_> = (0..32).map(|_| queue.try_acquire().unwrap()).collect();
    assert_eq!(queue.depth(), 32);
    assert_eq!(
        queue.try_acquire().err().unwrap().code,
        CommandErrorCode::Busy
    );
    drop(permits);
    assert_eq!(queue.depth(), 0);
}
#[test]
fn complete_utf8_delivery_at_catalog_limits_fits_256_kib_and_wrong_version_is_rejected() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json")).unwrap();
    let mut delivery: Delivery = serde_json::from_value(fixture["delivery"].clone()).unwrap();
    fn maximize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "name"
                            | "label"
                            | "hostname"
                            | "osName"
                            | "osVersion"
                            | "cpuModel"
                            | "scopeLabel"
                    ) && value.is_string()
                    {
                        *value = json!("界".repeat(42));
                    } else if matches!(key.as_str(), "reason" | "message" | "note")
                        && !value.is_null()
                    {
                        *value = json!("界".repeat(53));
                    } else {
                        maximize(value);
                    }
                }
            }
            Value::Array(values) => {
                for v in values {
                    maximize(v);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(&delivery).unwrap();
    maximize(&mut value);
    delivery = serde_json::from_value(value).unwrap();
    delivery.validate().unwrap();
    // Expand every bounded collection, including UTF-8 scope references, to its catalog limit.
    use meowndesk_lib::telemetry::model::*;
    let sample = delivery.view.sample.as_mut().unwrap();
    while sample.scopes.len() < 16 {
        let mut scope = sample.scopes[0].clone();
        scope.id = format!("scope-{:02}-{}", sample.scopes.len(), "界".repeat(39));
        scope.label = "界".repeat(42);
        scope.provider = "界".repeat(10);
        sample.scopes.push(scope);
    }
    while sample.capabilities.len() < 64 {
        let mut capability = sample.capabilities[0].clone();
        capability.metric_id =
            METRIC_CATALOG[sample.capabilities.len() % METRIC_CATALOG.len()].into();
        capability.scope_id = sample.scopes[15].id.clone();
        capability.provider = "界".repeat(10);
        capability.status = ReadingStatus::Unsupported;
        capability.reason = Some("界".repeat(53));
        sample.capabilities.push(capability);
    }
    let summaries = delivery.view.process_summaries.as_mut().unwrap();
    let prototype = summaries.cpu_top[0].clone();
    let list = |count: usize| {
        (0..count)
            .map(|i| {
                let mut p = prototype.clone();
                p.key = format!("{i:03}:{}", "界".repeat(41));
                p.name = "界".repeat(42);
                p.resident_bytes.value = Some(ByteCount(1));
                p.cpu_pct.value = Some(50.0);
                p.gpu_pct = Reading::valid(
                    50.0,
                    p.cpu_pct.sampled_at_ms.unwrap(),
                    p.cpu_pct.elapsed_ms.unwrap(),
                    sample.gpu.utilization_pct.scope_id.as_deref().unwrap(),
                );
                p.power_w = Reading::valid(
                    50.0,
                    p.cpu_pct.sampled_at_ms.unwrap(),
                    p.cpu_pct.elapsed_ms.unwrap(),
                    sample.power.watts.scope_id.as_deref().unwrap(),
                );
                p
            })
            .collect::<Vec<_>>()
    };
    summaries.memory_top = list(8);
    summaries.cpu_top = list(5);
    summaries.gpu_top = list(5);
    summaries.power_top = list(5);
    summaries.displayed_memory_total_bytes = ByteCount(8);
    summaries.coverage_notes = vec!["界".repeat(53); 8];
    for breakdown in [
        &mut delivery.view.memory_breakdowns.ram,
        &mut delivery.view.memory_breakdowns.vram,
    ]
    .into_iter()
    .flatten()
    {
        let prototype = breakdown.segments[0].clone();
        breakdown.total_bytes = ByteCount(u64::MAX);
        breakdown.coverage = BreakdownCoverage::Partial;
        breakdown.note = Some("界".repeat(53));
        breakdown.segments = (0..8)
            .map(|i| {
                let mut segment = prototype.clone();
                segment.id = format!("{i:03}:{}", "界".repeat(41));
                segment.label = "界".repeat(42);
                segment.bytes = ByteCount(u64::MAX / 8);
                segment
            })
            .collect();
    }
    delivery.view.notices = vec![
        Notice {
            code: "界".repeat(10),
            message: "界".repeat(53)
        };
        8
    ];
    let history = delivery.view.history.as_mut().unwrap();
    for chart in &mut history.charts {
        chart.scope_label = Some("界".repeat(42));
        for column in &mut chart.columns {
            if column.value.is_some() {
                column.sample_count = u32::MAX;
            }
        }
    }
    delivery.validate().unwrap();
    fn worst_text(max: usize) -> String {
        format!("{}é", "\"\\".repeat((max - 2) / 2))
    }
    let ids = delivery
        .view
        .sample
        .as_ref()
        .unwrap()
        .scopes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), format!("{i:03}:{}", worst_text(124))))
        .collect::<std::collections::BTreeMap<_, _>>();
    fn expand(value: &mut Value, ids: &std::collections::BTreeMap<String, String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "name"
                            | "label"
                            | "hostname"
                            | "osName"
                            | "osVersion"
                            | "cpuModel"
                            | "scopeLabel"
                    ) && value.is_string()
                    {
                        *value = json!(worst_text(128));
                    } else if matches!(key.as_str(), "provider" | "code") && value.is_string() {
                        *value = json!(worst_text(32));
                    } else if matches!(key.as_str(), "reason" | "message" | "note")
                        && !value.is_null()
                    {
                        *value = json!(worst_text(160));
                    } else {
                        expand(value, ids);
                    }
                }
            }
            Value::Array(values) => {
                for v in values {
                    expand(v, ids);
                }
            }
            Value::String(v) => {
                if let Some(new) = ids.get(v) {
                    *v = new.clone();
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(&delivery).unwrap();
    expand(&mut value, &ids);
    delivery = serde_json::from_value(value).unwrap();
    delivery.validate().unwrap();
    let bytes = serde_json::to_vec(&delivery).unwrap();
    assert!(bytes.len() <= 256 * 1024, "{} bytes", bytes.len());
    let metadata =
        meowndesk_lib::telemetry::bounds::network_member_metadata(&vec![worst_text(128); 40]);
    assert_eq!(metadata.member_ids.len(), 32);
    assert_eq!(metadata.omitted_member_count, 8);
    // Metadata stays internal until view construction defines its presentation. This conservative
    // bound also leaves room for its complete serialized representation without adding wire keys.
    let with_metadata = bytes.len() + serde_json::to_vec(&metadata).unwrap().len();
    assert!(with_metadata <= 256 * 1024);
    eprintln!(
        "Full bounded Delivery: {} UTF-8 bytes; with metadata allowance: {}",
        bytes.len(),
        with_metadata
    );
    delivery.contract_version = 2;
    assert!(delivery.validate().is_err());
}
// The live driver tests below are compiled now and deliberately fail at the missing subscribe
// command. T038 supplies the test-only Rust inspection seam, without new production IPC fields.
#[test]
fn subscribe_returns_an_owned_subscription_identifier() {
    let first = invoke("subscribe_telemetry", json!({"onView":"__CHANNEL__:1"}));
    assert_eq!(first["ok"], true);
    let id = &first["data"]["subscriptionId"];
    assert!(id.is_string());
    // Full same-owner lifecycle and exact ACK assertions use the feature-gated driver below.
}

#[cfg(feature = "test-fixtures")]
mod lifecycle {
    use super::*;
    use meowndesk_lib::telemetry::{
        model::{Delivery, View},
        providers::fixtures::LiveProbe,
    };
    use std::sync::{Arc, Mutex};
    struct Driver {
        _app: tauri::App<tauri::test::MockRuntime>,
        window: tauri::WebviewWindow<tauri::test::MockRuntime>,
        probe: Arc<LiveProbe>,
        received: Arc<Mutex<Vec<Delivery>>>,
    }
    impl Driver {
        fn new() -> Self {
            let probe = Arc::new(LiveProbe::default());
            let received = Arc::new(Mutex::new(Vec::new()));
            let capture = received.clone();
            let builder = tauri::test::mock_builder()
                .manage(probe.clone())
                .channel_interceptor(move |_, _, _, body| {
                    if let tauri::ipc::InvokeResponseBody::Json(json) = body
                        && let Ok(delivery) = serde_json::from_str::<Delivery>(json)
                    {
                        capture.lock().unwrap().push(delivery);
                    }
                    true
                });
            let app = meowndesk_lib::configure(builder).build(context()).unwrap();
            let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap();
            Self {
                _app: app,
                window,
                probe,
                received,
            }
        }
        fn invoke(&self, command: &str, args: Value) -> Value {
            tauri::test::get_ipc_response(
                &self.window,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: self.window.url().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            )
            .unwrap_or_else(|e| panic!("{command}: {e:?}"))
            .deserialize()
            .unwrap()
        }
        fn subscribe(&self, callback: u32) -> String {
            let result = self.invoke(
                "subscribe_telemetry",
                json!({"onView":format!("__CHANNEL__:{callback}")}),
            );
            assert_eq!(result["ok"], true);
            result["data"]["subscriptionId"].as_str().unwrap().into()
        }
        fn ack(&self, id: &str, seq: u64) -> bool {
            self.invoke(
                "ack_telemetry",
                json!({"subscriptionId":id,"deliverySequence":seq}),
            )["data"]["accepted"]
                .as_bool()
                .unwrap()
        }
        fn publish(&self, view: View) {
            self.probe.publish(view);
            let counts = self.probe.counts();
            assert!(counts.in_flight <= 1);
            assert!(counts.pending <= 1);
            assert!(counts.queued_controls <= 32);
            for d in self.received.lock().unwrap().iter() {
                assert!(serde_json::to_vec(d).unwrap().len() <= 256 * 1024);
            }
        }
        fn view(&self) -> View {
            let response = self.invoke("bootstrap", json!({}));
            assert_eq!(response["ok"], true);
            serde_json::from_value(response["data"]["view"].clone()).unwrap()
        }
    }
    #[test]
    fn exact_ack_zero_one_in_flight_one_replaceable_pending_and_no_replay() {
        let driver = Driver::new();
        let mut view = driver.view();
        let id = driver.subscribe(1);
        assert!(driver.received.lock().unwrap().is_empty());
        assert!(!driver.ack(&id, 1));
        driver.publish(view.clone());
        assert!(driver.received.lock().unwrap().is_empty());
        assert!(driver.ack(&id, 0));
        assert!(!driver.ack(&id, 0));
        assert_eq!(driver.received.lock().unwrap().len(), 1);
        let first = driver.received.lock().unwrap()[0].delivery_sequence;
        for _ in 0..100 {
            view.sample.as_mut().unwrap().sequence += 1;
            view.sample.as_mut().unwrap().elapsed_ms += 1000;
            view.sample.as_mut().unwrap().captured_at_ms += 1000;
            view.endpoint_elapsed_ms += 1000;
            view.endpoint_at_ms += 1000;
            view.history.as_mut().unwrap().endpoint_elapsed_ms = view.endpoint_elapsed_ms;
            driver.publish(view.clone());
        }
        let counts = driver.probe.counts();
        assert_eq!(counts.in_flight, 1);
        assert_eq!(counts.pending, 1);
        assert_eq!(driver.received.lock().unwrap().len(), 1);
        assert!(driver.ack(&id, first));
        assert!(!driver.ack(&id, first));
        assert_eq!(driver.received.lock().unwrap().len(), 2);
        assert_eq!(
            driver.received.lock().unwrap()[1]
                .view
                .sample
                .as_ref()
                .unwrap()
                .sequence,
            view.sample.as_ref().unwrap().sequence
        );
        assert_eq!(driver.probe.counts().pending, 0);
    }
    #[test]
    fn replacement_owner_unsubscribe_and_hidden_delivery_suppression() {
        let driver = Driver::new();
        let view = driver.view();
        let first = driver.subscribe(1);
        let second = driver.subscribe(2);
        assert_ne!(first, second);
        let result = driver.invoke(
            "ack_telemetry",
            json!({"subscriptionId":first,"deliverySequence":0}),
        );
        assert_eq!(result["error"]["code"], "invalid_subscription");
        let hidden = driver.invoke("set_delivery_visibility", json!({"visible":false}));
        assert_eq!(
            hidden["data"]["view"]["sample"]["sequence"],
            json!(view.sample.as_ref().unwrap().sequence)
        );
        assert!(driver.ack(&second, 0));
        driver.publish(view.clone());
        assert!(driver.received.lock().unwrap().is_empty());
        assert_eq!(driver.probe.counts().pending, 0);
        driver.invoke("set_delivery_visibility", json!({"visible":true}));
        driver.publish(view);
        let result = driver.invoke("unsubscribe_telemetry", json!({"subscriptionId":second}));
        assert_eq!(result["data"]["removed"], true);
        let result = driver.invoke("unsubscribe_telemetry", json!({"subscriptionId":second}));
        assert_eq!(result["data"]["removed"], false);
        assert_eq!(driver.probe.counts().in_flight, 0);
        assert_eq!(driver.probe.counts().pending, 0);
    }
}
