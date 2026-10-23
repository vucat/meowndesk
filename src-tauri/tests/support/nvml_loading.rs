//! Loader-policy regressions run as a unit module of the provider; no production test API.
use super::*;
fn missing_library() -> NvmlError {
    let missing =
        std::env::temp_dir().join(format!("meowndesk-absent-nvml-{}", std::process::id()));
    Nvml::builder()
        .lib_path(missing.as_os_str())
        .init()
        .expect_err("nonexistent library must fail to load")
}
#[test]
fn default_loader_success_never_tries_wsl_paths() {
    let mut attempts = Vec::new();
    let owner = initialize_with(|path| {
        attempts.push(path);
        Ok(7)
    })
    .unwrap();
    assert_eq!(owner, 7);
    assert_eq!(attempts, [None]);
}
#[cfg(target_os = "linux")]
#[test]
fn missing_default_tries_versioned_wsl_library_first() {
    let mut attempts = Vec::new();
    let owner = initialize_with(|path| {
        attempts.push(path);
        if path.is_none() {
            Err(missing_library())
        } else {
            Ok(7)
        }
    })
    .unwrap();
    assert_eq!(owner, 7);
    assert_eq!(
        attempts,
        [None, Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so.1"))]
    );
}
#[cfg(target_os = "linux")]
#[test]
fn missing_versioned_library_tries_unversioned_wsl_alias() {
    let mut attempts = Vec::new();
    let owner = initialize_with(|path| {
        attempts.push(path);
        if path == Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so")) {
            Ok(7)
        } else {
            Err(missing_library())
        }
    })
    .unwrap();
    assert_eq!(owner, 7);
    assert_eq!(
        attempts,
        [
            None,
            Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so.1")),
            Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so"))
        ]
    );
}
#[cfg(target_os = "linux")]
#[test]
fn missing_all_libraries_returns_original_load_error() {
    let mut attempts = Vec::new();
    let result: Result<(), _> = initialize_with(|path| {
        attempts.push(path);
        Err(missing_library())
    });
    assert_eq!(
        result.unwrap_err().to_string(),
        missing_library().to_string()
    );
    assert_eq!(attempts.len(), 3);
}
#[test]
fn missing_fallback_libraries_do_not_mask_default_permission_denial() {
    let mut attempts = 0;
    let result: Result<(), _> = initialize_with(|path| {
        attempts += 1;
        if path.is_none() {
            Err(NvmlError::NoPermission)
        } else {
            Err(missing_library())
        }
    });
    assert!(matches!(result, Err(NvmlError::NoPermission)));
    assert_eq!(attempts, if cfg!(target_os = "linux") { 3 } else { 1 });
}
#[cfg(target_os = "linux")]
#[test]
fn loaded_wsl_library_preserves_driver_error_when_alias_is_missing() {
    let mut attempts = Vec::new();
    let result: Result<(), _> = initialize_with(|path| {
        attempts.push(path);
        if path == Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so.1")) {
            Err(NvmlError::DriverNotLoaded)
        } else {
            Err(missing_library())
        }
    });
    assert!(matches!(result, Err(NvmlError::DriverNotLoaded)));
    assert_eq!(attempts.len(), 3);
}
#[cfg(target_os = "linux")]
#[test]
fn native_library_driver_not_loaded_can_recover_through_wsl_mount() {
    let mut attempts = Vec::new();
    let owner = initialize_with(|path| {
        attempts.push(path);
        if path.is_none() {
            Err(NvmlError::DriverNotLoaded)
        } else {
            Ok(7)
        }
    })
    .unwrap();
    assert_eq!(owner, 7);
    assert_eq!(
        attempts,
        [None, Some(OsStr::new("/usr/lib/wsl/lib/libnvidia-ml.so.1"))]
    );
}
