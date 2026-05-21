//! Safe identifier validation.
//!
//! Table and column names ARE interpolated into SQL (sqlx parameters do
//! not work for identifiers). Operator-supplied identifiers from the YAML
//! config pass through `SafeIdent::new` which enforces a strict
//! `[A-Za-z_][A-Za-z0-9_]*` shape — anything else is rejected before it
//! can land in a query.
//!
//! Two-component identifiers (e.g., `eh_demo.customers`) are validated as
//! two components joined by a single dot, each component matching the same
//! grammar.

use eh_connector_api::ConnectorError;

/// A validated SQL identifier safe to interpolate into a query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SafeIdent(String);

impl SafeIdent {
    /// Validate a single-component identifier (e.g., a column name).
    pub(crate) fn single(raw: &str) -> Result<Self, ConnectorError> {
        if Self::is_valid_component(raw) {
            Ok(Self(raw.to_string()))
        } else {
            Err(ConnectorError::InvalidIntent(format!(
                "invalid SQL identifier {raw:?}: must match [A-Za-z_][A-Za-z0-9_]*"
            )))
        }
    }

    /// Validate a one- or two-component identifier (e.g.,
    /// `customers` or `eh_demo.customers`).
    pub(crate) fn table(raw: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = raw.split('.').collect();
        match parts.len() {
            1 if Self::is_valid_component(parts[0]) => Ok(Self(raw.to_string())),
            2 if Self::is_valid_component(parts[0]) && Self::is_valid_component(parts[1]) => {
                Ok(Self(raw.to_string()))
            }
            _ => Err(ConnectorError::InvalidIntent(format!(
                "invalid SQL table identifier {raw:?}: must match `name` or `schema.name`"
            ))),
        }
    }

    fn is_valid_component(s: &str) -> bool {
        if s.is_empty() || s.len() > 64 {
            return false;
        }
        let mut chars = s.chars();
        let first = chars.next().expect("non-empty checked above");
        if !(first.is_ascii_alphabetic() || first == '_') {
            return false;
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// The validated identifier as it should appear in SQL.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_accepts_simple_identifier() {
        assert_eq!(
            SafeIdent::single("customers").unwrap().as_str(),
            "customers"
        );
        assert_eq!(SafeIdent::single("_under").unwrap().as_str(), "_under");
        assert_eq!(SafeIdent::single("a1_b2").unwrap().as_str(), "a1_b2");
    }

    #[test]
    fn single_rejects_dots() {
        assert!(SafeIdent::single("eh_demo.customers").is_err());
    }

    #[test]
    fn single_rejects_special_chars() {
        for bad in [
            "drop table",
            "id;",
            "id ",
            "id-",
            "1col",
            "",
            "id'or'1=1",
            "id\"",
            "id`",
        ] {
            assert!(SafeIdent::single(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn table_accepts_simple_and_qualified() {
        assert_eq!(SafeIdent::table("customers").unwrap().as_str(), "customers");
        assert_eq!(
            SafeIdent::table("eh_demo.customers").unwrap().as_str(),
            "eh_demo.customers"
        );
    }

    #[test]
    fn table_rejects_three_components() {
        assert!(SafeIdent::table("cat.eh_demo.customers").is_err());
    }

    #[test]
    fn table_rejects_empty_components() {
        assert!(SafeIdent::table(".customers").is_err());
        assert!(SafeIdent::table("eh_demo.").is_err());
        assert!(SafeIdent::table(".").is_err());
    }

    #[test]
    fn rejects_excessively_long_components() {
        let too_long = "a".repeat(65);
        assert!(SafeIdent::single(&too_long).is_err());
        assert!(SafeIdent::table(&too_long).is_err());
        let qualified = format!("eh_demo.{too_long}");
        assert!(SafeIdent::table(&qualified).is_err());
    }
}
