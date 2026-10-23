//! Version-1 preference persistence boundaries.
use meowndesk_lib::{
    preferences::{self, MAX_FILE_BYTES, defaults},
    telemetry::model::CollectionPreferences,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "meowndesk-prefs-{name}-{}-{unique}",
        std::process::id()
    ))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

#[test]
fn missing_file_uses_one_second_defaults_without_a_notice() {
    let directory = temp_path("missing");
    let loaded = preferences::load(&directory.join(preferences::FILE_NAME));
    assert_eq!(loaded.preferences, defaults());
    assert!(loaded.notice.is_none());
}

#[test]
fn version_one_roundtrip_ignores_unknown_keys_and_leaves_no_temp_file() {
    let directory = temp_path("roundtrip");
    let path = directory.join(preferences::FILE_NAME);
    let saved = CollectionPreferences {
        interval_ms: 5000,
        selected_gpu_id: Some("GPU-fixture-1".into()),
    };
    preferences::save(&path, &saved).unwrap();
    let loaded = preferences::load(&path);
    assert_eq!(loaded.preferences, saved);
    assert!(loaded.notice.is_none());
    assert!(!path.with_file_name("preferences.json.tmp").exists());

    let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["intervalMs"], 5000);
    assert_eq!(json["selectedGpuId"], "GPU-fixture-1");
    let mut extended = json.as_object().unwrap().clone();
    extended.insert("future".into(), serde_json::json!(true));
    fs::write(&path, serde_json::to_vec(&extended).unwrap()).unwrap();
    assert_eq!(preferences::load(&path).preferences, saved);
    cleanup(&directory);
}

#[test]
fn invalid_content_version_types_and_intervals_restore_defaults_with_a_notice() {
    let directory = temp_path("invalid");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(preferences::FILE_NAME);
    for content in [
        "not json".to_string(),
        r#"{"version":2,"intervalMs":1000,"selectedGpuId":null}"#.to_string(),
        r#"{"version":1,"intervalMs":3000,"selectedGpuId":null}"#.to_string(),
        r#"{"version":1,"intervalMs":"1000","selectedGpuId":null}"#.to_string(),
        r#"{"version":1,"intervalMs":1000,"selectedGpuId":""}"#.to_string(),
        r#"{"intervalMs":1000,"selectedGpuId":null}"#.to_string(),
    ] {
        fs::write(&path, content).unwrap();
        let loaded = preferences::load(&path);
        assert_eq!(loaded.preferences, defaults());
        assert!(loaded.notice.is_some());
    }
    cleanup(&directory);
}

#[test]
fn files_larger_than_sixteen_kib_are_rejected_without_parsing() {
    let directory = temp_path("large");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(preferences::FILE_NAME);
    let mut content = vec![b' '; MAX_FILE_BYTES + 1];
    content[0] = b'{';
    fs::write(&path, content).unwrap();
    let loaded = preferences::load(&path);
    assert_eq!(loaded.preferences, defaults());
    assert!(loaded.notice.is_some());
    cleanup(&directory);
}

#[test]
fn invalid_requests_and_unwritable_paths_fail_without_partial_writes() {
    let directory = temp_path("failure");
    fs::create_dir_all(&directory).unwrap();
    let blocker = directory.join("not-a-directory");
    fs::write(&blocker, b"file").unwrap();
    let path = blocker.join(preferences::FILE_NAME);
    assert!(preferences::save(&path, &defaults()).is_err());
    for interval_ms in [0, 999, 3000, 60_000] {
        assert!(
            preferences::save(
                &directory.join("x.json"),
                &CollectionPreferences {
                    interval_ms,
                    selected_gpu_id: None,
                }
            )
            .is_err()
        );
    }
    assert!(
        preferences::save(
            &directory.join("y.json"),
            &CollectionPreferences {
                interval_ms: 1000,
                selected_gpu_id: Some("bad\nid".into()),
            }
        )
        .is_err()
    );
    cleanup(&directory);
}

#[test]
fn replacing_an_existing_document_keeps_complete_readable_preferences() {
    let directory = temp_path("replace");
    let path = directory.join(preferences::FILE_NAME);
    preferences::save(&path, &defaults()).unwrap();
    for interval_ms in [2000, 5000, 1000] {
        let next = CollectionPreferences {
            interval_ms,
            selected_gpu_id: Some("GPU-replacement".into()),
        };
        preferences::save(&path, &next).unwrap();
        let loaded = preferences::load(&path);
        assert_eq!(loaded.preferences, next);
        assert!(loaded.notice.is_none());
        assert!(!path.with_file_name("preferences.json.tmp").exists());
    }
    cleanup(&directory);
}

#[cfg(unix)]
#[test]
fn failed_temporary_write_preserves_saved_preferences_and_cleans_the_temporary_file() {
    use std::os::unix::fs::PermissionsExt;
    let directory = temp_path("write-failure");
    let path = directory.join(preferences::FILE_NAME);
    preferences::save(&path, &defaults()).unwrap();
    let original = fs::read(&path).unwrap();
    let temporary = path.with_file_name("preferences.json.tmp");
    fs::write(&temporary, b"incomplete temporary content").unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o444)).unwrap();
    let result = preferences::save(
        &path,
        &CollectionPreferences {
            interval_ms: 5000,
            selected_gpu_id: None,
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(preferences::load(&path).preferences, defaults());
    let temporary_remains = temporary.exists();
    cleanup(&directory);
    assert!(
        !temporary_remains,
        "failed writes must not leave a temporary document"
    );
}

#[cfg(windows)]
#[test]
fn denied_windows_replacement_preserves_existing_document_and_cleans_temporary_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = temp_path("windows-replace-failure");
    let path = directory.join(preferences::FILE_NAME);
    preferences::save(&path, &defaults()).unwrap();
    let original = fs::read(&path).unwrap();
    // FILE_SHARE_READ permits inspection but denies deletion/replacement while held open.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let result = preferences::save(
        &path,
        &CollectionPreferences {
            interval_ms: 5000,
            selected_gpu_id: None,
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!path.with_file_name("preferences.json.tmp").exists());
    drop(held);
    cleanup(&directory);
}
