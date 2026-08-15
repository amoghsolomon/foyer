use std::{env, net::SocketAddr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeEnv {
    Development,
    Production,
}

impl RuntimeEnv {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }

    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("production") {
            "development" | "dev" => Ok(Self::Development),
            "production" | "prod" | "" => Ok(Self::Production),
            other => Err(format!(
                "invalid FOYER_SERVER_ENV {other:?}: expected development or production"
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevUser {
    pub user_id: String,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavSettings {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub runtime_env: RuntimeEnv,
    pub dev_users: Vec<DevUser>,
    pub powersync_url: Option<String>,
    pub powersync_audience: String,
    pub auth_signing_key_path: Option<String>,
    pub auth_key_id: String,
    pub auth_issuer: String,
    pub auth_api_audience: String,
    pub dav: Option<DavSettings>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_values(EnvValues::from_os())
    }

    fn from_values(values: EnvValues) -> Result<Self, String> {
        let bind = values
            .bind
            .unwrap_or_else(|| "127.0.0.1:3583".into())
            .parse()
            .map_err(|error| format!("invalid FOYER_SERVER_BIND: {error}"))?;
        let database_url = values
            .database_url
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "FOYER_DATABASE_URL is required".to_string())?;
        let runtime_env = RuntimeEnv::parse(values.runtime_env.as_deref())?;
        let powersync_audience = values
            .powersync_audience
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "foyer-powersync".into());
        let auth_signing_key_path = values
            .auth_signing_key_path
            .filter(|value| !value.is_empty());

        let (dev_users, auth_key_id, auth_issuer, auth_api_audience) = match runtime_env {
            RuntimeEnv::Development => {
                let user_id = required_dev("FOYER_DEV_USER_ID", values.dev_user_id)?;
                let token = required_dev("FOYER_DEV_TOKEN", values.dev_token)?;
                let mut users = vec![DevUser { user_id, token }];
                if let Some(extra) = values.dev_extra_users.filter(|value| !value.is_empty()) {
                    users.extend(parse_extra_users(&extra)?);
                }
                (
                    users,
                    values
                        .auth_key_id
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "foyer-dev".into()),
                    values
                        .auth_issuer
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "foyer-server".into()),
                    values
                        .auth_api_audience
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "foyer-api".into()),
                )
            }
            RuntimeEnv::Production => {
                if values
                    .dev_token
                    .as_deref()
                    .is_some_and(|token| !token.is_empty())
                {
                    return Err(
                        "FOYER_DEV_TOKEN is set but FOYER_SERVER_ENV is not development; refusing to start".into(),
                    );
                }
                if auth_signing_key_path.is_none() {
                    return Err(
                        "FOYER_AUTH_SIGNING_KEY_PATH is required when FOYER_SERVER_ENV is not development"
                            .into(),
                    );
                }
                (
                    Vec::new(),
                    required_value("FOYER_AUTH_KEY_ID", values.auth_key_id)?,
                    required_value("FOYER_AUTH_ISSUER", values.auth_issuer)?,
                    required_value("FOYER_AUTH_API_AUDIENCE", values.auth_api_audience)?,
                )
            }
        };
        if auth_api_audience == powersync_audience {
            return Err(
                "FOYER_AUTH_API_AUDIENCE and FOYER_POWERSYNC_AUDIENCE must be distinct".into(),
            );
        }

        let dav = Some(DavSettings {
            base_url: required_value("FOYER_DAV_URL", values.dav_url)?,
            username: required_value("FOYER_DAV_USERNAME", values.dav_username)?,
            password: required_value("FOYER_DAV_PASSWORD", values.dav_password)?,
        });

        Ok(Self {
            bind,
            database_url,
            runtime_env,
            dev_users,
            powersync_url: values.powersync_url.filter(|value| !value.is_empty()),
            powersync_audience,
            auth_signing_key_path,
            auth_key_id,
            auth_issuer,
            auth_api_audience,
            dav,
        })
    }

    pub fn is_development(&self) -> bool {
        self.runtime_env == RuntimeEnv::Development
    }

    pub fn user_for_token(&self, token: &str) -> Option<&DevUser> {
        if !self.is_development() {
            return None;
        }
        self.dev_users.iter().find(|user| user.token == token)
    }

    pub fn test_development(database_url: impl Into<String>) -> Self {
        Self {
            bind: "127.0.0.1:3583".parse().unwrap(),
            database_url: database_url.into(),
            runtime_env: RuntimeEnv::Development,
            dev_users: vec![DevUser {
                user_id: "dev-user".into(),
                token: "dev-token".into(),
            }],
            powersync_url: Some("http://127.0.0.1:8080".into()),
            powersync_audience: "foyer-powersync".into(),
            auth_signing_key_path: None,
            auth_key_id: "foyer-dev".into(),
            auth_issuer: "foyer-server".into(),
            auth_api_audience: "foyer-api".into(),
            dav: None,
        }
    }
}

struct EnvValues {
    bind: Option<String>,
    database_url: Option<String>,
    runtime_env: Option<String>,
    dev_user_id: Option<String>,
    dev_token: Option<String>,
    dev_extra_users: Option<String>,
    powersync_url: Option<String>,
    powersync_audience: Option<String>,
    auth_signing_key_path: Option<String>,
    auth_key_id: Option<String>,
    auth_issuer: Option<String>,
    auth_api_audience: Option<String>,
    dav_url: Option<String>,
    dav_username: Option<String>,
    dav_password: Option<String>,
}

impl EnvValues {
    fn from_os() -> Self {
        Self {
            bind: env::var("FOYER_SERVER_BIND").ok(),
            database_url: env::var("FOYER_DATABASE_URL").ok(),
            runtime_env: env::var("FOYER_SERVER_ENV").ok(),
            dev_user_id: env::var("FOYER_DEV_USER_ID").ok(),
            dev_token: env::var("FOYER_DEV_TOKEN").ok(),
            dev_extra_users: env::var("FOYER_DEV_EXTRA_USERS").ok(),
            powersync_url: env::var("FOYER_POWERSYNC_URL").ok(),
            powersync_audience: env::var("FOYER_POWERSYNC_AUDIENCE").ok(),
            auth_signing_key_path: env::var("FOYER_AUTH_SIGNING_KEY_PATH").ok(),
            auth_key_id: env::var("FOYER_AUTH_KEY_ID").ok(),
            auth_issuer: env::var("FOYER_AUTH_ISSUER").ok(),
            auth_api_audience: env::var("FOYER_AUTH_API_AUDIENCE").ok(),
            dav_url: env::var("FOYER_DAV_URL").ok(),
            dav_username: env::var("FOYER_DAV_USERNAME").ok(),
            dav_password: env::var("FOYER_DAV_PASSWORD").ok(),
        }
    }
}

fn required_dev(name: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required when FOYER_SERVER_ENV=development"))
}

fn required_value(name: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn parse_extra_users(value: &str) -> Result<Vec<DevUser>, String> {
    let mut users = Vec::new();
    for entry in value.split(';').filter(|entry| !entry.is_empty()) {
        let (user_id, token) = entry.split_once('=').ok_or_else(|| {
            format!("invalid FOYER_DEV_EXTRA_USERS entry {entry:?}: expected user_id=token")
        })?;
        if user_id.is_empty() || token.is_empty() {
            return Err(format!("invalid FOYER_DEV_EXTRA_USERS entry {entry:?}"));
        }
        users.push(DevUser {
            user_id: user_id.to_string(),
            token: token.to_string(),
        });
    }
    Ok(users)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn development_values() -> EnvValues {
        EnvValues {
            bind: None,
            database_url: Some("postgres://foyer:foyer@127.0.0.1:5432/foyer".into()),
            runtime_env: Some("development".into()),
            dev_user_id: Some("dev-user".into()),
            dev_token: Some("dev-token".into()),
            dev_extra_users: None,
            powersync_url: Some("http://127.0.0.1:8080".into()),
            powersync_audience: None,
            auth_signing_key_path: None,
            auth_key_id: None,
            auth_issuer: None,
            auth_api_audience: None,
            dav_url: Some("http://radicale:5232".into()),
            dav_username: Some("foyer".into()),
            dav_password: Some("foyer-dev-dav-password-do-not-use-outside-development".into()),
        }
    }

    #[test]
    fn default_bind_is_loopback() {
        let config = Config::from_values(development_values()).expect("config");
        assert_eq!(config.bind, "127.0.0.1:3583".parse().unwrap());
        assert!(config.is_development());
    }

    #[test]
    fn development_without_token_fails_closed() {
        let mut values = development_values();
        values.dev_token = None;
        assert!(Config::from_values(values).is_err());
    }

    #[test]
    fn production_with_dev_token_fails_closed() {
        let mut values = development_values();
        values.runtime_env = Some("production".into());
        assert!(Config::from_values(values).is_err());
    }

    #[test]
    fn production_without_signing_key_fails_closed() {
        let values = EnvValues {
            bind: None,
            database_url: Some("postgres://foyer:foyer@127.0.0.1:5432/foyer".into()),
            runtime_env: Some("production".into()),
            dev_user_id: None,
            dev_token: None,
            dev_extra_users: None,
            powersync_url: None,
            powersync_audience: None,
            auth_signing_key_path: None,
            auth_key_id: Some("prod".into()),
            auth_issuer: Some("foyer-server".into()),
            auth_api_audience: Some("foyer-api".into()),
            dav_url: Some("http://radicale:5232".into()),
            dav_username: Some("foyer".into()),
            dav_password: Some("production-dav-password".into()),
        };
        assert!(Config::from_values(values).is_err());
    }

    #[test]
    fn production_with_signing_configuration_starts() {
        let values = EnvValues {
            bind: None,
            database_url: Some("postgres://foyer:foyer@127.0.0.1:5432/foyer".into()),
            runtime_env: Some("production".into()),
            dev_user_id: None,
            dev_token: None,
            dev_extra_users: None,
            powersync_url: None,
            powersync_audience: None,
            auth_signing_key_path: Some("/secrets/foyer-auth.pem".into()),
            auth_key_id: Some("foyer-2026-01".into()),
            auth_issuer: Some("https://foyer.example".into()),
            auth_api_audience: Some("foyer-api".into()),
            dav_url: Some("http://radicale:5232".into()),
            dav_username: Some("foyer".into()),
            dav_password: Some("production-dav-password".into()),
        };
        let config = Config::from_values(values).expect("production config");
        assert!(!config.is_development());
        assert!(config.user_for_token("dev-token").is_none());
        assert_eq!(config.auth_key_id, "foyer-2026-01");
        assert_eq!(config.auth_api_audience, "foyer-api");
        assert_eq!(config.powersync_audience, "foyer-powersync");
        assert_eq!(config.dav.as_ref().unwrap().username, "foyer");
    }

    #[test]
    fn identical_audiences_fail_closed() {
        let mut values = development_values();
        values.auth_api_audience = Some("foyer-powersync".into());
        values.powersync_audience = Some("foyer-powersync".into());
        assert!(Config::from_values(values).is_err());
    }

    #[test]
    fn missing_dav_settings_fail_closed() {
        let mut values = development_values();
        values.dav_password = None;
        assert!(Config::from_values(values).is_err());
    }

    #[test]
    fn extra_development_users_are_parsed() {
        let mut values = development_values();
        values.dev_extra_users = Some("other=other-token".into());
        let config = Config::from_values(values).expect("config");
        assert_eq!(
            config.user_for_token("other-token").unwrap().user_id,
            "other"
        );
    }
}
