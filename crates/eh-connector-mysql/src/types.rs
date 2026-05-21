//! Type-safe conversion between JSON wire values and `sqlx::MySql` row /
//! parameter slots.
//!
//! The conversion is *typed* end-to-end per zero-trust §14: each entity
//! field carries a `FieldType` (UUID, Timestamp, Decimal, …) and the
//! conversion functions enforce it on both sides. A JSON value whose shape
//! does not match the declared `FieldType` produces a
//! `ConnectorError::TypeMismatch`, never a silent coercion.
//!
//! All binding / extraction happens through native sqlx types — no
//! `::text` casts, no `UUID_TO_BIN()` shims, no `format!()`-built SQL.

use chrono::{DateTime, NaiveDateTime, Utc};
use eh_connector_api::ConnectorError;
use eh_core::FieldType;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::mysql::MySqlArguments;
use sqlx::Arguments;
use uuid::Uuid;

/// Extracted strongly-typed value to bind into a query.
///
/// Each variant carries a Rust type that `sqlx::MySql` knows natively. The
/// enum exists so the caller can pre-validate before binding and bubble up
/// a typed error.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BindValue {
    String(String),
    I32(i32),
    I64(i64),
    F64(f64),
    Bool(bool),
    Uuid(Uuid),
    Timestamp(NaiveDateTime),
    Decimal(Decimal),
    Json(Value),
    Bytes(Vec<u8>),
    Null,
}

impl BindValue {
    /// Decode a JSON wire value into a `BindValue` matching the declared
    /// entity field type. The conversion is strict — a JSON string fed
    /// where the field declares an `Int` produces `TypeMismatch`.
    pub(crate) fn from_json(
        field_name: &str,
        field_type: FieldType,
        value: &Value,
    ) -> Result<Self, ConnectorError> {
        if value.is_null() {
            return Ok(BindValue::Null);
        }
        match field_type {
            FieldType::String | FieldType::Text => match value {
                Value::String(s) => Ok(BindValue::String(s.clone())),
                _ => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Int => match value.as_i64() {
                Some(n) if (i32::MIN as i64..=i32::MAX as i64).contains(&n) => {
                    Ok(BindValue::I32(n as i32))
                }
                _ => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::BigInt => match value.as_i64() {
                Some(n) => Ok(BindValue::I64(n)),
                None => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Float => match value.as_f64() {
                Some(n) => Ok(BindValue::F64(n)),
                None => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Bool => match value.as_bool() {
                Some(b) => Ok(BindValue::Bool(b)),
                None => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Uuid => match value {
                Value::String(s) => Uuid::parse_str(s)
                    .map(BindValue::Uuid)
                    .map_err(|_| mismatch(field_name, field_type, value)),
                _ => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Timestamp => match value {
                Value::String(s) => parse_timestamp(s)
                    .ok_or_else(|| mismatch(field_name, field_type, value))
                    .map(BindValue::Timestamp),
                _ => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Decimal => match value {
                Value::String(s) => s
                    .parse::<Decimal>()
                    .map(BindValue::Decimal)
                    .map_err(|_| mismatch(field_name, field_type, value)),
                Value::Number(n) => n
                    .to_string()
                    .parse::<Decimal>()
                    .map(BindValue::Decimal)
                    .map_err(|_| mismatch(field_name, field_type, value)),
                _ => Err(mismatch(field_name, field_type, value)),
            },
            FieldType::Json => Ok(BindValue::Json(value.clone())),
            FieldType::Binary => match value {
                Value::String(s) => hex::decode(s)
                    .map(BindValue::Bytes)
                    .map_err(|_| mismatch(field_name, field_type, value)),
                _ => Err(mismatch(field_name, field_type, value)),
            },
        }
    }

    /// Bind this value as the next positional argument in a query.
    pub(crate) fn bind_into(&self, args: &mut MySqlArguments) -> Result<(), ConnectorError> {
        let bind = |result: Result<(), sqlx::error::BoxDynError>| {
            result.map_err(|e| ConnectorError::Backend(format!("bind failed: {e}")))
        };
        match self {
            BindValue::String(s) => bind(args.add(s.clone())),
            BindValue::I32(n) => bind(args.add(*n)),
            BindValue::I64(n) => bind(args.add(*n)),
            BindValue::F64(n) => bind(args.add(*n)),
            BindValue::Bool(b) => bind(args.add(*b)),
            BindValue::Uuid(u) => bind(args.add(*u)),
            BindValue::Timestamp(ts) => bind(args.add(*ts)),
            BindValue::Decimal(d) => bind(args.add(*d)),
            BindValue::Json(j) => bind(args.add(sqlx::types::Json(j.clone()))),
            BindValue::Bytes(b) => bind(args.add(b.clone())),
            BindValue::Null => bind(args.add(Option::<String>::None)),
        }
    }
}

fn parse_timestamp(s: &str) -> Option<NaiveDateTime> {
    // Accept RFC 3339 (with optional Z / tz) AND naive forms.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).naive_utc());
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(ts) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(ts);
        }
    }
    None
}

fn mismatch(field: &str, expected: FieldType, value: &Value) -> ConnectorError {
    ConnectorError::TypeMismatch {
        field: field.to_string(),
        expected: format!("{expected:?}").to_lowercase(),
        actual: actual_type_for(value).to_string(),
    }
}

fn actual_type_for(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Decode a sqlx row column into the JSON value the artifact carries.
///
/// Each `FieldType` maps to the native Rust type sqlx uses, then to a JSON
/// representation. UUIDs come out as hyphenated strings; timestamps as
/// RFC 3339 in UTC; decimals as strings (to preserve precision); binaries
/// as lowercase hex strings.
pub(crate) fn decode_column(
    row: &sqlx::mysql::MySqlRow,
    physical_col: &str,
    field_name: &str,
    field_type: FieldType,
) -> Result<Value, ConnectorError> {
    use sqlx::Row as _;

    let try_get_or_mismatch = |actual: &'static str| -> ConnectorError {
        ConnectorError::TypeMismatch {
            field: field_name.to_string(),
            expected: format!("{field_type:?}").to_lowercase(),
            actual: actual.to_string(),
        }
    };

    match field_type {
        FieldType::String | FieldType::Text => {
            let v: Option<String> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("string-decode"))?;
            Ok(v.map(Value::String).unwrap_or(Value::Null))
        }
        FieldType::Int => {
            let v: Option<i32> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("int-decode"))?;
            Ok(v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null))
        }
        FieldType::BigInt => {
            let v: Option<i64> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("bigint-decode"))?;
            Ok(v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null))
        }
        FieldType::Float => {
            let v: Option<f64> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("float-decode"))?;
            Ok(v.and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or(Value::Null))
        }
        FieldType::Bool => {
            let v: Option<bool> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("bool-decode"))?;
            Ok(v.map(Value::Bool).unwrap_or(Value::Null))
        }
        FieldType::Uuid => {
            let v: Option<Uuid> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("uuid-decode"))?;
            Ok(v.map(|u| Value::String(u.to_string()))
                .unwrap_or(Value::Null))
        }
        FieldType::Timestamp => {
            let v: Option<NaiveDateTime> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("timestamp-decode"))?;
            Ok(
                v.map(|ts| Value::String(ts.format("%Y-%m-%dT%H:%M:%S%.f").to_string()))
                    .unwrap_or(Value::Null),
            )
        }
        FieldType::Decimal => {
            let v: Option<Decimal> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("decimal-decode"))?;
            Ok(v.map(|d| Value::String(d.to_string()))
                .unwrap_or(Value::Null))
        }
        FieldType::Json => {
            let v: Option<sqlx::types::Json<Value>> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("json-decode"))?;
            Ok(v.map(|j| j.0).unwrap_or(Value::Null))
        }
        FieldType::Binary => {
            let v: Option<Vec<u8>> = row
                .try_get(physical_col)
                .map_err(|_| try_get_or_mismatch("bytes-decode"))?;
            Ok(v.map(|b| Value::String(hex::encode(b)))
                .unwrap_or(Value::Null))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_json_accepts_string_for_string_type() {
        let v = BindValue::from_json("name", FieldType::String, &json!("Alice")).unwrap();
        assert!(matches!(v, BindValue::String(ref s) if s == "Alice"));
    }

    #[test]
    fn from_json_rejects_number_for_string_type() {
        let err = BindValue::from_json("name", FieldType::String, &json!(42)).unwrap_err();
        assert!(matches!(err, ConnectorError::TypeMismatch { .. }));
    }

    #[test]
    fn from_json_uuid_parses() {
        let v = BindValue::from_json(
            "id",
            FieldType::Uuid,
            &json!("01914a01-7001-7001-8001-000000000001"),
        )
        .unwrap();
        match v {
            BindValue::Uuid(u) => assert_eq!(u.to_string(), "01914a01-7001-7001-8001-000000000001"),
            _ => panic!("expected Uuid"),
        }
    }

    #[test]
    fn from_json_uuid_rejects_garbage() {
        let err = BindValue::from_json("id", FieldType::Uuid, &json!("not-a-uuid")).unwrap_err();
        assert!(matches!(err, ConnectorError::TypeMismatch { .. }));
    }

    #[test]
    fn from_json_timestamp_accepts_rfc3339_and_naive() {
        let cases = [
            "2024-09-01T09:00:00Z",
            "2024-09-01T09:00:00+00:00",
            "2024-09-01T09:00:00",
            "2024-09-01 09:00:00",
            "2024-09-01 09:00:00.123",
        ];
        for s in cases {
            let v = BindValue::from_json("signup_at", FieldType::Timestamp, &json!(s)).unwrap();
            assert!(matches!(v, BindValue::Timestamp(_)), "failed on {s}");
        }
    }

    #[test]
    fn from_json_timestamp_rejects_garbage() {
        let err = BindValue::from_json("signup_at", FieldType::Timestamp, &json!("yesterday"))
            .unwrap_err();
        assert!(matches!(err, ConnectorError::TypeMismatch { .. }));
    }

    #[test]
    fn from_json_decimal_accepts_string_and_number() {
        let v1 = BindValue::from_json("ltv", FieldType::Decimal, &json!("1250.00")).unwrap();
        assert!(matches!(v1, BindValue::Decimal(_)));
        let v2 = BindValue::from_json("ltv", FieldType::Decimal, &json!(1250)).unwrap();
        assert!(matches!(v2, BindValue::Decimal(_)));
    }

    #[test]
    fn from_json_int_range_checked() {
        let ok = BindValue::from_json("n", FieldType::Int, &json!(42)).unwrap();
        assert!(matches!(ok, BindValue::I32(42)));
        let too_big = BindValue::from_json("n", FieldType::Int, &json!(i64::MAX)).unwrap_err();
        assert!(matches!(too_big, ConnectorError::TypeMismatch { .. }));
    }

    #[test]
    fn from_json_null_is_null_for_any_type() {
        let v = BindValue::from_json("x", FieldType::String, &Value::Null).unwrap();
        assert!(matches!(v, BindValue::Null));
        let v = BindValue::from_json("x", FieldType::Uuid, &Value::Null).unwrap();
        assert!(matches!(v, BindValue::Null));
    }

    #[test]
    fn bind_into_accepts_each_variant() {
        let mut args = MySqlArguments::default();
        BindValue::String("a".into()).bind_into(&mut args).unwrap();
        BindValue::I32(1).bind_into(&mut args).unwrap();
        BindValue::I64(2).bind_into(&mut args).unwrap();
        BindValue::F64(1.5).bind_into(&mut args).unwrap();
        BindValue::Bool(true).bind_into(&mut args).unwrap();
        BindValue::Uuid(Uuid::nil()).bind_into(&mut args).unwrap();
        BindValue::Timestamp(NaiveDateTime::default())
            .bind_into(&mut args)
            .unwrap();
        BindValue::Decimal(Decimal::ZERO)
            .bind_into(&mut args)
            .unwrap();
        BindValue::Json(json!({})).bind_into(&mut args).unwrap();
        BindValue::Bytes(vec![0xde, 0xad])
            .bind_into(&mut args)
            .unwrap();
        BindValue::Null.bind_into(&mut args).unwrap();
    }
}
