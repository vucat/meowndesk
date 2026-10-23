//! Version-1 local preferences. Only interval and the stable GPU identity are persisted.
use crate::telemetry::{bounds, model::CollectionPreferences};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const FILE_NAME: &str = "preferences.json";
pub const MAX_FILE_BYTES: usize = 16 * 1024;
pub const VALID_INTERVALS: [u64; 3] = [1000, 2000, 5000];

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    version: u32,
    interval_ms: u64,
    #[serde(default)]
    selected_gpu_id: Option<String>,
}

pub struct Loaded {
    pub preferences: CollectionPreferences,
    pub notice: Option<String>,
}

pub fn defaults() -> CollectionPreferences {
    CollectionPreferences {
        interval_ms: 1000,
        selected_gpu_id: None,
    }
}

pub fn validate(preferences: &CollectionPreferences) -> bool {
    VALID_INTERVALS.contains(&preferences.interval_ms)
        && preferences.selected_gpu_id.as_deref().is_none_or(|id| {
            !id.is_empty()
                && bounds::is_within(id, bounds::LABEL_MAX_BYTES)
                && !id.chars().any(char::is_control)
        })
}

fn invalid_notice(detail: &str) -> String {
    bounds::bounded_message(&format!(
        "Saved preferences were reset to defaults: {detail}"
    ))
}

/// Read the fixed preference file. Missing files are silent defaults; invalid content is
/// replaced by defaults with a curated notice.
pub fn load(path: &Path) -> Loaded {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Loaded {
                preferences: defaults(),
                notice: None,
            };
        }
        Err(_) => {
            return Loaded {
                preferences: defaults(),
                notice: Some(invalid_notice("the file could not be read")),
            };
        }
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Loaded {
            preferences: defaults(),
            notice: Some(invalid_notice("the file exceeds 16 KiB")),
        };
    }
    let stored: Stored = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => {
            return Loaded {
                preferences: defaults(),
                notice: Some(invalid_notice("the file is not valid version-1 JSON")),
            };
        }
    };
    if stored.version != 1 {
        return Loaded {
            preferences: defaults(),
            notice: Some(invalid_notice("the version is not supported")),
        };
    }
    let preferences = CollectionPreferences {
        interval_ms: stored.interval_ms,
        selected_gpu_id: stored.selected_gpu_id,
    };
    if !validate(&preferences) {
        return Loaded {
            preferences: defaults(),
            notice: Some(invalid_notice("one or more values are invalid")),
        };
    }
    Loaded {
        preferences,
        notice: None,
    }
}

pub fn save(path: &Path, preferences: &CollectionPreferences) -> io::Result<()> {
    if !validate(preferences) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preferences are outside the version-1 domain",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stored = Stored {
        version: 1,
        interval_ms: preferences.interval_ms,
        selected_gpu_id: preferences.selected_gpu_id.clone(),
    };
    let json = serde_json::to_vec(&stored)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temporary = temporary_path(path);
    // Same-directory rename replaces an existing file on Unix and Windows. Never delete the
    // destination: if replacement fails, the last valid document must remain readable.
    let result = fs::write(&temporary, json).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| FILE_NAME.into());
    name.push_str(".tmp");
    path.with_file_name(name)
}
