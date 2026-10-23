//! Production handler, generated ACL, strict arguments and local-content isolation.
use serde_json::{Value, json};

const COMMANDS: [&str; 8] = [
    "bootstrap",
    "subscribe_telemetry",
    "ack_telemetry",
    "unsubscribe_telemetry",
    "set_view",
    "set_paused",
    "set_preferences",
    "set_delivery_visibility",
];
fn response(label: &str, url: Option<&str>, command: &str, args: Value) -> Result<Value, Value> {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = meowndesk_lib::configure(tauri::test::mock_builder())
        .build(context)
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
        .build()
        .unwrap();
    tauri::test::get_ipc_response(
        &window,
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: url
                .map(|u| u.parse().unwrap())
                .unwrap_or_else(|| window.url().unwrap()),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    )
    .map(|value| value.deserialize().unwrap())
}
#[test]
fn only_local_main_can_call_every_registered_command() {
    assert_eq!(
        response("main", None, "bootstrap", json!({})).unwrap()["ok"],
        true
    );
    for command in COMMANDS {
        assert!(
            response("untrusted", None, command, json!({})).is_err(),
            "{command} allowed outside main"
        );
        assert!(
            response("main", Some("https://example.invalid/"), command, json!({})).is_err(),
            "{command} allowed remotely"
        );
    }
}
#[test]
fn fixture_shell_filesystem_and_process_commands_are_not_registered() {
    for command in [
        "set_fixture",
        "install_probe",
        "fixture_control",
        "kill_process",
        "read_file",
        "execute",
        "plugin:shell|execute",
        "plugin:fs|read_file",
        "plugin:http|fetch",
    ] {
        assert!(
            response("main", None, command, json!({})).is_err(),
            "unexpected command {command}"
        );
    }
    let handler = include_str!("../src/lib.rs");
    assert!(!handler.contains("providers::fixtures"));
    let provider_modules = include_str!("../src/telemetry/providers/mod.rs");
    assert!(provider_modules.contains("#[cfg(feature = \"test-fixtures\")]\npub mod fixtures;"));
}
#[test]
fn command_inputs_reject_paths_process_control_unknown_fields_and_invalid_enums() {
    for (command, args) in [
        ("bootstrap", json!({"path":"/etc/passwd"})),
        ("set_paused", json!({"paused":true,"pid":1})),
        (
            "set_view",
            json!({"selection":{"range":"2h","memoryView":"ram","rankingMetric":"cpu"}}),
        ),
        (
            "set_preferences",
            json!({"intervalMs":1000,"selectedGpuId":null,"path":"preferences.json"}),
        ),
        (
            "set_preferences",
            json!({"intervalMs":3000,"selectedGpuId":null}),
        ),
    ] {
        assert_eq!(
            response("main", None, command, args).unwrap()["error"]["code"],
            "invalid_argument"
        );
    }
}
#[test]
fn production_policy_is_local_and_capability_grants_only_telemetry() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let csp = config["app"]["security"]["csp"].as_str().unwrap();
    assert!(csp.contains("connect-src ipc: http://ipc.localhost"));
    for forbidden in ["https:", "unsafe-eval", "unsafe-inline", "*"] {
        assert!(!csp.contains(forbidden));
    }
    let capability: Value =
        serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
    assert_eq!(capability["windows"], json!(["main"]));
    assert_eq!(capability["permissions"], json!(["telemetry"]));
    assert!(capability.get("remote").is_none());
    assert_ne!(capability.get("local"), Some(&json!(false)));
    let manifest = include_str!("../build.rs");
    for command in COMMANDS {
        assert!(manifest.contains(&format!("\"{command}\"")));
    }
}
