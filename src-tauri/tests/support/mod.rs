pub fn shell_config() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tauri.conf.json"))
        .expect("the native shell configuration must be valid JSON")
}
