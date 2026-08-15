//! Canonical Foyer Shell filesystem roots and the one-time product-rename migration.

use std::{env, fs, path::PathBuf};

const PRODUCT_DIRECTORY: &str = "foyer-shell";
const LEGACY_PRODUCT_DIRECTORY: &str = "amazity-shell";

pub fn data_root() -> PathBuf {
    let parent = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(env::temp_dir);
    migrate_directory(&parent, LEGACY_PRODUCT_DIRECTORY, PRODUCT_DIRECTORY);
    parent.join(PRODUCT_DIRECTORY)
}

/// Shared PowerSync replica for hosted personal data. Never the Foyer Shell storage database.
pub fn personal_replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_PERSONAL_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root().join("personal-powersync.sqlite3"))
}

pub fn config_root() -> PathBuf {
    let parent = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(env::temp_dir);
    migrate_directory(&parent, LEGACY_PRODUCT_DIRECTORY, PRODUCT_DIRECTORY);
    parent.join(PRODUCT_DIRECTORY)
}

pub fn state_root() -> PathBuf {
    let parent = env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .unwrap_or_else(env::temp_dir);
    migrate_directory(&parent, LEGACY_PRODUCT_DIRECTORY, PRODUCT_DIRECTORY);
    parent.join(PRODUCT_DIRECTORY)
}

/// Operator-readable public device enrollment document. Never the private key.
pub fn device_enrollment_path() -> PathBuf {
    env::var_os("FOYER_SHELL_DEVICE_ENROLLMENT_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state_root().join("device-enrollment.json"))
}

fn migrate_directory(parent: &std::path::Path, legacy_name: &str, current_name: &str) {
    let legacy = parent.join(legacy_name);
    let current = parent.join(current_name);
    if current.exists() || !legacy.is_dir() {
        return;
    }
    if let Err(error) = fs::rename(&legacy, &current) {
        eprintln!(
            "Foyer Shell could not migrate {} to {}: {error}",
            legacy.display(),
            current.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_directory_names_are_distinct() {
        assert_eq!(PRODUCT_DIRECTORY, "foyer-shell");
        assert_eq!(LEGACY_PRODUCT_DIRECTORY, "amazity-shell");
    }

    #[test]
    fn enrollment_file_is_under_state_root_by_default() {
        assert!(device_enrollment_path().ends_with("device-enrollment.json"));
    }
}
