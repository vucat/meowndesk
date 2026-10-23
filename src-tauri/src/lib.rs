pub mod ipc;
pub mod preferences;
pub mod telemetry;
use std::sync::Arc;
use tauri::Manager;
use telemetry::collector::TelemetryService;

/// The same command/ACL configuration is used by production and Tauri mock-runtime tests.
pub fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .manage(Arc::new(TelemetryService::new()))
        .invoke_handler(tauri::generate_handler![
            ipc::bootstrap,
            ipc::subscribe_telemetry,
            ipc::ack_telemetry,
            ipc::unsubscribe_telemetry,
            ipc::set_delivery_visibility,
            ipc::set_view,
            ipc::set_paused,
            ipc::set_preferences
        ])
        .setup(|app| {
            let service = app.state::<Arc<TelemetryService>>().inner().clone();
            if let Ok(directory) = app.path().app_config_dir() {
                let path = directory.join(preferences::FILE_NAME);
                let loaded = preferences::load(&path);
                service.initialize_preferences(path, loaded.preferences, loaded.notice);
            }
            service.start();
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let service = window.state::<Arc<TelemetryService>>();
            match event {
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                    service.close()
                }
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(_) => {
                    if let (Ok(visible), Ok(minimized)) =
                        (window.is_visible(), window.is_minimized())
                    {
                        service.native_visibility(visible && !minimized);
                    }
                }
                _ => {}
            }
        })
}
pub fn run() {
    let app = match configure(tauri::Builder::default()).build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(_) => {
            eprintln!("Meowndesk could not initialize the desktop window");
            std::process::exit(1);
        }
    };
    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            app.state::<Arc<TelemetryService>>().close();
        }
    });
}
