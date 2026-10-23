fn main() {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "bootstrap",
            "subscribe_telemetry",
            "ack_telemetry",
            "unsubscribe_telemetry",
            "set_view",
            "set_paused",
            "set_preferences",
            "set_delivery_visibility",
        ]));
    tauri_build::try_build(attributes).expect("failed to run tauri-build");

    // Tauri embeds the app manifest in the main binary, but integration tests
    // that link its mock runtime also import Common Controls v6 functions.
    // Without their own manifest they fail before the test harness starts:
    // https://github.com/tauri-apps/tauri/issues/13419
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("windows")
    ) && matches!(std::env::var("CARGO_CFG_TARGET_ENV").as_deref(), Ok("msvc"))
    {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(concat!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:",
            "type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' ",
            "processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        ));
    }
}
