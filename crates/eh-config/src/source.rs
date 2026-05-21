//! Opaque source configuration.
//!
//! `eh-config` is intentionally **connector-agnostic**: it parses the YAML
//! source block as a `kind` string + the remaining keys as a generic YAML
//! mapping. The mapping is **opaque** to this crate. The connector for
//! that kind is responsible for deserialising the mapping into its own
//! typed config struct.
//!
//! This is the public extension path per zero-trust §15: adding a new
//! connector kind requires NO change to `eh-config` (or any other kernel
//! crate). The connector author owns their config schema; the kernel
//! holds only the type-erased shape.

use serde::de::{Deserializer, Error as DeError, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::fmt;

/// A registered source. The connector's typed config is captured here as
/// an opaque `serde_yaml::Mapping` — the connector parses it via its own
/// struct at `ConnectorFactory::build` time.
///
/// YAML shape (the operator authors this):
///
/// ```yaml
/// sources:
///   fvp_mysql:
///     kind: mysql
///     # … any other keys the mysql connector declares …
///     host: mysql
///     port: 3306
///     database: eh_demo
///     username: eh_service
///     password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
/// ```
///
/// `eh-config` extracts `kind` and treats the rest as the connector's
/// private config. No per-kind variants live in this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceConfig {
    /// Lowercase kind identifier matching `eh_control.sources.kind`. The
    /// `ConnectorRegistry` looks up the factory by this string.
    pub kind: String,

    /// Everything else from the YAML block. Includes connector-specific
    /// fields. Does NOT include the `kind` field itself.
    pub raw: Mapping,
}

impl SourceConfig {
    /// Construct directly. Used by tests and by callers that build configs
    /// programmatically.
    #[must_use]
    pub fn new(kind: impl Into<String>, raw: Mapping) -> Self {
        Self {
            kind: kind.into(),
            raw,
        }
    }

    /// Deserialise the opaque config into a connector-supplied typed struct.
    ///
    /// Each connector calls this from its `ConnectorFactory::build` impl.
    pub fn parse_into<T>(&self) -> Result<T, serde_yaml::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_yaml::from_value(Value::Mapping(self.raw.clone()))
    }
}

// ---------------------------------------------------------------------------
// Custom (de)serialisation: the YAML is FLAT — `kind` is one of the keys at
// the same level as the connector-specific fields. We extract `kind` and
// retain the rest as the opaque mapping. There is no nested `config:` key
// to mirror; the operator-facing shape stays clean.
// ---------------------------------------------------------------------------

impl Serialize for SourceConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.raw.len() + 1))?;
        map.serialize_entry("kind", &self.kind)?;
        for (k, v) in &self.raw {
            // Defence in depth: never let a stray `kind:` inside `raw`
            // double-emit. (`raw` should not carry kind, but the type
            // does not enforce it.)
            if let Some(s) = k.as_str() {
                if s == "kind" {
                    continue;
                }
            }
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SourceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SourceVisitor;

        impl<'de> Visitor<'de> for SourceVisitor {
            type Value = SourceConfig;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a YAML mapping with at least a `kind:` field")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut kind: Option<String> = None;
                let mut raw = Mapping::new();

                while let Some(key) = access.next_key::<Value>()? {
                    let value = access.next_value::<Value>()?;
                    if let Some(s) = key.as_str() {
                        if s == "kind" {
                            let k = value
                                .as_str()
                                .ok_or_else(|| DeError::custom("source `kind` must be a string"))?;
                            kind = Some(k.to_string());
                            continue;
                        }
                    }
                    raw.insert(key, value);
                }

                let kind = kind.ok_or_else(|| DeError::custom("source missing `kind`"))?;
                Ok(SourceConfig { kind, raw })
            }
        }

        deserializer.deserialize_map(SourceVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;

    #[test]
    fn parse_flat_yaml_extracts_kind_and_retains_rest() {
        let yaml = r#"
kind: mysql
host: mysql
port: 3306
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
"#;
        let s: SourceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(s.kind, "mysql");
        assert!(s.raw.contains_key(Value::String("host".into())));
        assert!(s.raw.contains_key(Value::String("port".into())));
        assert!(s.raw.contains_key(Value::String("database".into())));
        assert!(s.raw.contains_key(Value::String("password".into())));
        // `kind` is NOT in raw.
        assert!(!s.raw.contains_key(Value::String("kind".into())));
    }

    #[test]
    fn missing_kind_is_rejected() {
        let yaml = r#"
host: mysql
database: eh_demo
"#;
        let err = serde_yaml::from_str::<SourceConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("kind"));
    }

    #[test]
    fn kind_not_a_string_is_rejected() {
        let yaml = r#"
kind: 42
host: mysql
"#;
        let err = serde_yaml::from_str::<SourceConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("kind"));
    }

    #[test]
    fn parse_into_with_typed_struct_works() {
        // Simulate a connector-supplied typed config struct.
        #[derive(Debug, Deserialize, PartialEq)]
        struct DemoMysqlConfig {
            host: String,
            port: u16,
            database: String,
            username: String,
        }

        let yaml = r#"
kind: mysql
host: mysql
port: 3306
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
"#;
        let src: SourceConfig = serde_yaml::from_str(yaml).unwrap();
        let typed: DemoMysqlConfig = src.parse_into().unwrap();
        assert_eq!(typed.host, "mysql");
        assert_eq!(typed.port, 3306);
        assert_eq!(typed.database, "eh_demo");
        assert_eq!(typed.username, "eh_service");
    }

    #[test]
    fn round_trip_via_yaml_preserves_kind_and_raw() {
        let mut raw = Mapping::new();
        raw.insert(Value::String("host".into()), Value::String("mysql".into()));
        raw.insert(Value::String("port".into()), Value::Number(3306.into()));
        let src = SourceConfig::new("mysql", raw);

        let yaml = serde_yaml::to_string(&src).unwrap();
        let back: SourceConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(src, back);
        // The serialised YAML carries `kind:` and the raw keys, no nesting.
        assert!(yaml.contains("kind: mysql"));
        assert!(yaml.contains("host: mysql"));
        assert!(yaml.contains("port: 3306"));
    }

    #[test]
    fn serialise_skips_stray_kind_inside_raw() {
        // Construct with a `kind` key inside raw — this should NOT appear in
        // the YAML because the top-level kind is always authoritative.
        let mut raw = Mapping::new();
        raw.insert(
            Value::String("kind".into()),
            Value::String("imposter".into()),
        );
        raw.insert(Value::String("host".into()), Value::String("h".into()));
        let src = SourceConfig::new("mysql", raw);
        let yaml = serde_yaml::to_string(&src).unwrap();
        assert!(yaml.contains("kind: mysql"));
        assert!(!yaml.contains("imposter"));
    }
}
