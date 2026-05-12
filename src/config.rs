//! Config types: TOML schema, env interpolation, URI parsing.
//!
//! See spec section "Config Schema" for the user-facing shape.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    #[serde(rename = "connection", default)]
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub views: ViewsConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct UiConfig {
    pub theme: String,
    pub tick_ms: u64,
    pub page_size: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            tick_ms: 2000,
            page_size: 100,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(default)]
pub struct ViewsConfig {
    #[serde(default)]
    pub queries: Option<ViewOverride>,
    #[serde(default)]
    pub locks: Option<ViewOverride>,
    #[serde(default)]
    pub sessions: Option<ViewOverride>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ViewOverride {
    pub tick_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConnectionConfig {
    pub name: String,
    /// Either `url = "postgres://..."` OR the discrete fields below.
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub database: Option<String>,
    pub password: Option<String>,
    pub sslmode: Option<String>,
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    pub fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::from_toml(&text)
    }

    /// Write the current config back to the given path, preserving everything.
    /// We re-serialize via `toml::to_string_pretty`, so formatting may change,
    /// but the data is preserved.
    pub fn save(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let contents =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(path, contents).map_err(ConfigError::Io)?;
        Ok(())
    }

    pub fn find_connection(&self, name: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.name == name)
    }
}

impl ConnectionConfig {
    /// Returns a clone with `${ENV_VAR}` placeholders in `password` and `url`
    /// resolved against the current process env.
    pub fn resolve_secrets(&self) -> Result<Self, ConfigError> {
        let mut out = self.clone();
        if let Some(p) = out.password.take() {
            out.password = Some(interpolate(&p)?);
        }
        if let Some(u) = out.url.take() {
            out.url = Some(interpolate(&u)?);
        }
        Ok(out)
    }

    /// Render this connection as a tokio_postgres-compatible connection string.
    /// Prefers `url` if present; otherwise builds a libpq-style key=value string.
    /// Caller should usually `resolve_secrets()` first.
    pub fn as_target(&self) -> Result<String, ConfigError> {
        if let Some(u) = &self.url {
            return Ok(u.clone());
        }
        let host = self
            .host
            .as_deref()
            .ok_or_else(|| ConfigError::BadUri("connection needs either `url` or `host`".into()))?;
        let mut parts = vec![format!("host={host}")];
        if let Some(p) = self.port {
            parts.push(format!("port={p}"));
        }
        if let Some(u) = &self.user {
            parts.push(format!("user={u}"));
        }
        if let Some(d) = &self.database {
            parts.push(format!("dbname={d}"));
        }
        if let Some(pw) = &self.password {
            parts.push(format!("password={pw}"));
        }
        if let Some(s) = &self.sslmode {
            parts.push(format!("sslmode={s}"));
        }
        parts.push("application_name=postui".to_string());
        Ok(parts.join(" "))
    }
}

/// Replace `${VAR}` occurrences with env values. Errors on first missing var.
fn interpolate(input: &str) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| ConfigError::Parse(format!("unclosed ${{ in {input:?}")))?;
        let var = &after[..end];
        let val = std::env::var(var).map_err(|_| ConfigError::MissingEnv {
            var: var.to_string(),
        })?;
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.ui.theme, "default");
        assert_eq!(cfg.ui.tick_ms, 2000);
        assert_eq!(cfg.ui.page_size, 100);
        assert!(cfg.connections.is_empty());
    }

    #[test]
    fn parses_full_example() {
        let toml = r#"
            [ui]
            theme = "dracula"
            tick_ms = 1500
            page_size = 50

            [[connection]]
            name = "local"
            host = "localhost"
            port = 5432
            user = "andrew"
            database = "app_dev"

            [[connection]]
            name = "stage"
            url = "postgres://andrew@db.stage:5432/app"

            [views.queries]
            tick_ms = 1000
        "#;
        let cfg = Config::from_toml(toml).unwrap();
        assert_eq!(cfg.ui.theme, "dracula");
        assert_eq!(cfg.ui.tick_ms, 1500);
        assert_eq!(cfg.ui.page_size, 50);
        assert_eq!(cfg.connections.len(), 2);
        assert_eq!(cfg.connections[0].name, "local");
        assert_eq!(
            cfg.connections[1].url.as_deref(),
            Some("postgres://andrew@db.stage:5432/app")
        );
        assert_eq!(
            cfg.views.queries.as_ref().and_then(|v| v.tick_ms),
            Some(1000)
        );
    }

    #[test]
    fn find_connection_works() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "prod"
            host = "h"
            user = "u"
            database = "d"
        "#,
        )
        .unwrap();
        assert!(cfg.find_connection("prod").is_some());
        assert!(cfg.find_connection("missing").is_none());
    }

    #[test]
    fn parse_errors_surface_message() {
        let err = Config::from_toml("[ui\nbroken").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn interpolates_env_in_password() {
        // SAFETY: tests are isolated; we set + unset.
        unsafe {
            std::env::set_var("POSTUI_TEST_PW", "s3cret");
        }
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "${POSTUI_TEST_PW}"
        "#,
        )
        .unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.password.as_deref(), Some("s3cret"));
        unsafe {
            std::env::remove_var("POSTUI_TEST_PW");
        }
    }

    #[test]
    fn interpolates_env_in_url() {
        unsafe {
            std::env::set_var("POSTUI_TEST_URL", "postgres://a:b@h/d");
        }
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            url = "${POSTUI_TEST_URL}"
        "#,
        )
        .unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.url.as_deref(), Some("postgres://a:b@h/d"));
        unsafe {
            std::env::remove_var("POSTUI_TEST_URL");
        }
    }

    #[test]
    fn missing_env_var_errors() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "${POSTUI_DEFINITELY_NOT_SET}"
        "#,
        )
        .unwrap();
        let err = cfg.connections[0].resolve_secrets().unwrap_err();
        match err {
            ConfigError::MissingEnv { var } => assert_eq!(var, "POSTUI_DEFINITELY_NOT_SET"),
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn passthrough_when_no_placeholder() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "literal-pw"
        "#,
        )
        .unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.password.as_deref(), Some("literal-pw"));
    }

    #[test]
    fn as_target_from_url() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            url = "postgres://u:p@h:5432/d"
        "#,
        )
        .unwrap();
        let target = cfg.connections[0].as_target().unwrap();
        assert!(target.contains("postgres://"));
        assert!(target.contains("u"));
        assert!(target.contains("h"));
    }

    #[test]
    fn as_target_from_fields() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
            host = "h"
            port = 5433
            user = "u"
            database = "d"
            password = "pw"
            sslmode = "require"
        "#,
        )
        .unwrap();
        let target = cfg.connections[0].as_target().unwrap();
        // libpq-style key=value string
        assert!(target.contains("host=h"));
        assert!(target.contains("port=5433"));
        assert!(target.contains("user=u"));
        assert!(target.contains("dbname=d"));
        assert!(target.contains("password=pw"));
        assert!(target.contains("sslmode=require"));
    }

    #[test]
    fn as_target_with_no_fields_errors() {
        let cfg = Config::from_toml(
            r#"
            [[connection]]
            name = "x"
        "#,
        )
        .unwrap();
        let err = cfg.connections[0].as_target().unwrap_err();
        assert!(matches!(err, ConfigError::BadUri(_)));
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut cfg = Config::default();
        cfg.ui.theme = "nord".into();
        cfg.connections.push(ConnectionConfig {
            name: "x".into(),
            url: None,
            host: Some("h".into()),
            port: Some(5432),
            user: Some("u".into()),
            database: Some("d".into()),
            password: None,
            sslmode: None,
        });
        cfg.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.ui.theme, "nord");
        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.connections[0].name, "x");
        assert_eq!(loaded.connections[0].host.as_deref(), Some("h"));
        assert_eq!(loaded.connections[0].port, Some(5432));
        assert_eq!(loaded.connections[0].user.as_deref(), Some("u"));
        assert_eq!(loaded.connections[0].database.as_deref(), Some("d"));
    }
}
