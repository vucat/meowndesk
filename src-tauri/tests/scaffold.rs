mod support;

#[test]
fn desktop_shell_has_one_local_window_at_the_supported_minimum_size() {
    let config = support::shell_config();
    let windows = config["app"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["label"], "main");
    assert_eq!(windows[0]["minWidth"], 900);
    assert_eq!(windows[0]["minHeight"], 640);
    let frontend = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(config["build"]["frontendDist"].as_str().unwrap());
    assert!(
        frontend.join("index.html").is_file(),
        "build the frontend before native tests"
    );
}
