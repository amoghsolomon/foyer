use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::AuthError;
use crate::jwk::EnrollmentMaterial;

pub fn default_enrollment_path() -> PathBuf {
    foyer_shell_paths::device_enrollment_path()
}

pub fn write_public_enrollment(
    path: &Path,
    material: &EnrollmentMaterial,
) -> Result<(), AuthError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AuthError::Protocol(format!("create enrollment directory: {error}"))
        })?;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }

    let payload = material.enrollment_json();
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|error| AuthError::Protocol(format!("write enrollment file: {error}")))?;
        file.write_all(payload.as_bytes())
            .map_err(|error| AuthError::Protocol(format!("write enrollment file: {error}")))?;
        file.sync_all()
            .map_err(|error| AuthError::Protocol(format!("write enrollment file: {error}")))?;
    }
    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        AuthError::Protocol(format!("publish enrollment file: {error}"))
    })?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    tracing::info!(
        path = %path.display(),
        "wrote Foyer device public enrollment file"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::PublicJwk;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_public_json_with_restrictive_permissions() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("foyer-auth-enroll-{unique}"));
        let path = dir.join("device-enrollment.json");
        let material = EnrollmentMaterial::new(
            PublicJwk::p256(
                "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
                "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
            )
            .expect("jwk"),
        );
        write_public_enrollment(&path, &material).expect("write");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s"));
        assert!(!text.contains("\"d\""));
        let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(dir);
    }
}
