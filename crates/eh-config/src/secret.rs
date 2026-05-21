//! Secret references and resolved secrets.
//!
//! Config files reference secret values via the form `${ENV:NAME}`. The
//! `SecretRef` parser captures the env-var name; the loader resolves it
//! against the process environment at load time and stores the result in a
//! `Secret` newtype whose `Debug` implementation never reveals the value.

use std::env;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::errors::{ConfigError, ConfigResult};

/// A reference to a secret in the operator's secrets manager.
///
/// Today only the `env://NAME` style is supported (resolved against the
/// process environment, which compose / Helm populate from the operator's
/// secrets manager). The grammar is small on purpose: configs declare
/// `${ENV:NAME}` and the parser captures `NAME`. Future schemes
/// (`vault://path`, `k8s://ns/name`) plug in here without touching call
/// sites.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum SecretRef {
    /// `${ENV:NAME}` — read from process env at load time.
    Env(String),
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretRef::Env(name) => write!(f, "SecretRef::Env({name})"),
        }
    }
}

impl SecretRef {
    /// Parse a reference of the form `${ENV:NAME}`.
    ///
    /// Plain (un-quoted) values are also accepted as `Env(name)` only when
    /// they are bare identifiers of the form `^[A-Z_][A-Z0-9_]*$`. Anything
    /// else fails to parse — embedding raw secrets in the config is a
    /// violation of zero-trust §13.
    pub fn parse(raw: &str) -> ConfigResult<Self> {
        if let Some(rest) = raw.strip_prefix("${ENV:").and_then(|r| r.strip_suffix('}')) {
            if rest.is_empty() {
                return Err(ConfigError::InvalidSecretRef(raw.to_string()));
            }
            return Ok(SecretRef::Env(rest.to_string()));
        }
        Err(ConfigError::InvalidSecretRef(raw.to_string()))
    }

    /// Resolve this reference to a concrete `Secret`.
    pub fn resolve(&self) -> ConfigResult<Secret> {
        match self {
            SecretRef::Env(name) => {
                let value = env::var(name).map_err(|_| ConfigError::MissingEnvVar(name.clone()))?;
                if value.is_empty() {
                    return Err(ConfigError::EmptyEnvVar(name.clone()));
                }
                Ok(Secret(value))
            }
        }
    }

    /// Return the env-var name this ref will read from.
    #[must_use]
    pub fn env_name(&self) -> &str {
        match self {
            SecretRef::Env(name) => name.as_str(),
        }
    }
}

impl TryFrom<String> for SecretRef {
    type Error = ConfigError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<SecretRef> for String {
    fn from(value: SecretRef) -> Self {
        match value {
            SecretRef::Env(name) => format!("${{ENV:{name}}}"),
        }
    }
}

/// A resolved secret value. Implements `Debug` redaction so the value never
/// shows up in logs or panic messages.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Construct directly from a string. Used in tests; production code
    /// arrives here through `SecretRef::resolve`.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expose the underlying value. Use sparingly — only at the boundary
    /// where the secret must be handed to a downstream API (e.g.,
    /// `sqlx::PgPoolOptions::connect_with`).
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_ref() {
        let r = SecretRef::parse("${ENV:FOO_BAR}").unwrap();
        assert_eq!(r.env_name(), "FOO_BAR");
    }

    #[test]
    fn parse_rejects_empty_name() {
        let r = SecretRef::parse("${ENV:}");
        assert!(r.is_err());
    }

    #[test]
    fn parse_rejects_raw_value() {
        let r = SecretRef::parse("hunter2");
        assert!(matches!(r, Err(ConfigError::InvalidSecretRef(_))));
    }

    #[test]
    fn resolve_missing_env_var_fails() {
        // Use an unlikely env-var name.
        let r = SecretRef::Env("EH_TEST_DEFINITELY_UNSET_VAR_8d2f".to_string());
        let err = r.resolve().unwrap_err();
        assert!(matches!(err, ConfigError::MissingEnvVar(_)));
    }

    #[test]
    #[allow(unsafe_code)]
    fn resolve_present_env_var_succeeds() {
        // env::set_var / env::remove_var are unsafe since Rust 1.84 because
        // they are not thread-safe. We use a unique per-test name so other
        // tests cannot race on the same variable. Production code never
        // calls them.
        let name = "EH_TEST_PRESENT_8d2f";
        // SAFETY: unique name; no concurrent reader/writer of this env var.
        unsafe { env::set_var(name, "verysecret") };
        let r = SecretRef::Env(name.to_string());
        let s = r.resolve().unwrap();
        assert_eq!(s.expose(), "verysecret");
        // SAFETY: same as above.
        unsafe { env::remove_var(name) };
    }

    #[test]
    #[allow(unsafe_code)]
    fn resolve_empty_env_var_fails() {
        let name = "EH_TEST_EMPTY_8d2f";
        // SAFETY: unique name; no concurrent access.
        unsafe { env::set_var(name, "") };
        let r = SecretRef::Env(name.to_string());
        let err = r.resolve().unwrap_err();
        assert!(matches!(err, ConfigError::EmptyEnvVar(_)));
        // SAFETY: same as above.
        unsafe { env::remove_var(name) };
    }

    #[test]
    fn secret_debug_redacts_value() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
    }

    #[test]
    fn secret_ref_debug_keeps_name_not_value() {
        let r = SecretRef::Env("PGPASSWORD".to_string());
        assert_eq!(format!("{r:?}"), "SecretRef::Env(PGPASSWORD)");
    }

    #[test]
    fn secret_ref_serialises_back_to_dollar_brace_form() {
        let r = SecretRef::Env("MYVAR".to_string());
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#""${ENV:MYVAR}""#);
    }

    #[test]
    fn secret_ref_round_trip_via_yaml() {
        let yaml = "${ENV:FOO}";
        let r: SecretRef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(r.env_name(), "FOO");
        let back = serde_yaml::to_string(&r).unwrap();
        assert!(back.contains("${ENV:FOO}"));
    }
}
