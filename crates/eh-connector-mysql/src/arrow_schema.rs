//! MySQL ↔ Arrow type mapping.
//!
//! Used by `introspection` (to convert `INFORMATION_SCHEMA.COLUMNS` rows
//! into Arrow `Field`s) and by `table_provider` (to materialise
//! `sqlx::MySqlRow` batches into `RecordBatch`es).
//!
//! Coverage: the common MySQL 8 base types. Unknown types surface a
//! typed error so the operator sees the unsupported column and can
//! either narrow the table's scope or wait for a connector update.

use std::sync::Arc;

use chrono::NaiveDateTime;
use datafusion::arrow::array::{
    ArrayRef, BooleanBuilder, Date32Builder, Decimal128Builder, Float32Builder, Float64Builder,
    Int16Builder, Int32Builder, Int64Builder, Int8Builder, LargeBinaryBuilder, StringBuilder,
    TimestampMicrosecondBuilder, UInt32Builder,
};
use datafusion::arrow::datatypes::{DataType, Field, TimeUnit};
use eh_connector_api::{ConnectorError, ConnectorResult};
use rust_decimal::Decimal;
use sqlx::{mysql::MySqlRow, Row};

/// Column descriptor as the connector understands it after introspecting
/// `INFORMATION_SCHEMA.COLUMNS`. Owns enough information to:
/// 1. Build the Arrow `Field` for this column (`arrow_field()`).
/// 2. Decode a `sqlx::MySqlRow` slot into the appropriate Arrow builder
///    (`append_to_builder()`).
#[derive(Debug, Clone)]
pub(crate) struct MysqlColumn {
    /// Column name as it appears in MySQL.
    pub name: String,
    /// `data_type` from `INFORMATION_SCHEMA.COLUMNS` (e.g., `varchar`,
    /// `bigint`, `decimal`, `datetime`). Lowercase.
    pub data_type: String,
    /// `column_type` from `INFORMATION_SCHEMA.COLUMNS` — the full type
    /// including precision (e.g., `decimal(10,2)`, `tinyint(1)`).
    pub column_type: String,
    /// True when the column allows `NULL`.
    pub nullable: bool,
    /// For DECIMAL columns. `None` for others.
    pub decimal_precision: Option<u8>,
    /// For DECIMAL columns. `None` for others.
    pub decimal_scale: Option<i8>,
}

impl MysqlColumn {
    /// Arrow `Field` describing this column.
    pub fn arrow_field(&self) -> ConnectorResult<Field> {
        let dt = self.arrow_data_type()?;
        Ok(Field::new(&self.name, dt, self.nullable))
    }

    /// Arrow `DataType` corresponding to the MySQL `data_type`.
    fn arrow_data_type(&self) -> ConnectorResult<DataType> {
        match self.data_type.as_str() {
            "tinyint" => {
                // Convention: TINYINT(1) is a boolean.
                if self.column_type == "tinyint(1)" {
                    Ok(DataType::Boolean)
                } else {
                    Ok(DataType::Int8)
                }
            }
            "smallint" => Ok(DataType::Int16),
            "mediumint" | "int" | "integer" => Ok(DataType::Int32),
            "bigint" => Ok(DataType::Int64),
            "year" => Ok(DataType::UInt32),
            "float" => Ok(DataType::Float32),
            "double" | "real" => Ok(DataType::Float64),
            "decimal" | "numeric" => {
                let precision = self.decimal_precision.unwrap_or(38);
                let scale = self.decimal_scale.unwrap_or(0);
                Ok(DataType::Decimal128(precision, scale))
            }
            "date" => Ok(DataType::Date32),
            "datetime" | "timestamp" => Ok(DataType::Timestamp(TimeUnit::Microsecond, None)),
            "char" | "varchar" | "text" | "tinytext" | "mediumtext" | "longtext" | "enum"
            | "set" | "json" => Ok(DataType::Utf8),
            "binary" | "varbinary" | "blob" | "tinyblob" | "mediumblob" | "longblob"
            | "geometry" => Ok(DataType::LargeBinary),
            other => Err(ConnectorError::Backend(format!(
                "MySQL column {:?}: unsupported data_type {other:?} (column_type {:?}). \
                 Narrow the source access scope to exclude this column or wait for a \
                 connector update that supports it.",
                self.name, self.column_type
            ))),
        }
    }
}

/// Convert one fetched MySQL row's value for `col_idx` into the matching
/// Arrow builder slot.
///
/// The builders are addressed by Arrow column index (in projection order).
/// `column` is the MySQL column descriptor for the same column.
#[allow(clippy::too_many_lines)]
pub(crate) fn append_value(
    row: &MySqlRow,
    column: &MysqlColumn,
    builder: &mut ArrayBuilderHandle,
) -> ConnectorResult<()> {
    let col_name = column.name.as_str();
    let backend_err = |ctx: &str| ConnectorError::Backend(format!("{col_name}: {ctx}"));

    match builder {
        ArrayBuilderHandle::Boolean(b) => {
            let v: Option<bool> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Int8(b) => {
            let v: Option<i8> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Int16(b) => {
            let v: Option<i16> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Int32(b) => {
            let v: Option<i32> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Int64(b) => {
            let v: Option<i64> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::UInt32(b) => {
            let v: Option<u32> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Float32(b) => {
            let v: Option<f32> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Float64(b) => {
            let v: Option<f64> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(x) => b.append_value(x),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Decimal128(b, table_scale) => {
            let v: Option<Decimal> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(d) => {
                    // Arrow Decimal128 stores values at a FIXED scale; we
                    // re-scale the incoming sqlx value (which may carry
                    // its own scale per-value) to the table's declared
                    // scale. Overflow surfaces a typed error.
                    let table_scale = i32::from(*table_scale);
                    let value_scale = d.scale() as i32;
                    let mantissa = d.mantissa();
                    let scaled = if value_scale <= table_scale {
                        let diff = (table_scale - value_scale) as u32;
                        mantissa.checked_mul(10i128.pow(diff)).ok_or_else(|| {
                            backend_err(&format!(
                                "decimal overflow scaling {d} up to scale {table_scale}"
                            ))
                        })?
                    } else {
                        let diff = (value_scale - table_scale) as u32;
                        mantissa / 10i128.pow(diff)
                    };
                    b.append_value(scaled);
                }
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Date32(b) => {
            let v: Option<chrono::NaiveDate> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(d) => {
                    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
                    let days = (d - epoch).num_days();
                    b.append_value(days as i32);
                }
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::TimestampMicros(b) => {
            let v: Option<NaiveDateTime> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(ts) => {
                    let micros = ts.and_utc().timestamp_micros();
                    b.append_value(micros);
                }
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::Utf8(b) => {
            // Most string-like columns. UUID columns stored as BINARY(16)
            // are handled via the Binary path; UUIDs stored as CHAR(36)
            // arrive here as plain strings.
            let v: Option<String> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(s) => b.append_value(s),
                None => b.append_null(),
            }
        }
        ArrayBuilderHandle::LargeBinary(b) => {
            let v: Option<Vec<u8>> = row
                .try_get(col_name)
                .map_err(|e| backend_err(&e.to_string()))?;
            match v {
                Some(bytes) => b.append_value(bytes),
                None => b.append_null(),
            }
        }
    }
    Ok(())
}

/// Concrete Arrow builder, one variant per supported `DataType`.
/// Exists so `append_value` can dispatch on the column's data type
/// without trait objects.
pub(crate) enum ArrayBuilderHandle {
    Boolean(BooleanBuilder),
    Int8(Int8Builder),
    Int16(Int16Builder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    UInt32(UInt32Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    /// Holds the builder + the table's declared decimal scale (used to
    /// re-scale incoming `rust_decimal::Decimal` values into the
    /// fixed-scale `Decimal128`).
    Decimal128(Decimal128Builder, i8),
    Date32(Date32Builder),
    TimestampMicros(TimestampMicrosecondBuilder),
    Utf8(StringBuilder),
    LargeBinary(LargeBinaryBuilder),
}

impl ArrayBuilderHandle {
    /// Construct a fresh builder for `dt` with `capacity` row slots.
    pub fn for_data_type(dt: &DataType, capacity: usize) -> ConnectorResult<Self> {
        Ok(match dt {
            DataType::Boolean => Self::Boolean(BooleanBuilder::with_capacity(capacity)),
            DataType::Int8 => Self::Int8(Int8Builder::with_capacity(capacity)),
            DataType::Int16 => Self::Int16(Int16Builder::with_capacity(capacity)),
            DataType::Int32 => Self::Int32(Int32Builder::with_capacity(capacity)),
            DataType::Int64 => Self::Int64(Int64Builder::with_capacity(capacity)),
            DataType::UInt32 => Self::UInt32(UInt32Builder::with_capacity(capacity)),
            DataType::Float32 => Self::Float32(Float32Builder::with_capacity(capacity)),
            DataType::Float64 => Self::Float64(Float64Builder::with_capacity(capacity)),
            DataType::Decimal128(precision, scale) => Self::Decimal128(
                Decimal128Builder::with_capacity(capacity)
                    .with_precision_and_scale(*precision, *scale)
                    .map_err(|e| {
                        ConnectorError::Backend(format!("decimal builder init failed: {e}"))
                    })?,
                *scale,
            ),
            DataType::Date32 => Self::Date32(Date32Builder::with_capacity(capacity)),
            DataType::Timestamp(TimeUnit::Microsecond, None) => {
                Self::TimestampMicros(TimestampMicrosecondBuilder::with_capacity(capacity))
            }
            DataType::Utf8 => Self::Utf8(StringBuilder::with_capacity(capacity, capacity * 32)),
            DataType::LargeBinary => {
                Self::LargeBinary(LargeBinaryBuilder::with_capacity(capacity, capacity * 32))
            }
            other => {
                return Err(ConnectorError::Backend(format!(
                    "no Arrow builder for data type {other:?}"
                )))
            }
        })
    }

    /// Finalise the builder into an `ArrayRef`.
    pub fn finish(self) -> ArrayRef {
        match self {
            Self::Boolean(mut b) => Arc::new(b.finish()),
            Self::Int8(mut b) => Arc::new(b.finish()),
            Self::Int16(mut b) => Arc::new(b.finish()),
            Self::Int32(mut b) => Arc::new(b.finish()),
            Self::Int64(mut b) => Arc::new(b.finish()),
            Self::UInt32(mut b) => Arc::new(b.finish()),
            Self::Float32(mut b) => Arc::new(b.finish()),
            Self::Float64(mut b) => Arc::new(b.finish()),
            Self::Decimal128(mut b, _) => Arc::new(b.finish()),
            Self::Date32(mut b) => Arc::new(b.finish()),
            Self::TimestampMicros(mut b) => Arc::new(b.finish()),
            Self::Utf8(mut b) => Arc::new(b.finish()),
            Self::LargeBinary(mut b) => Arc::new(b.finish()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, data_type: &str, column_type: &str, nullable: bool) -> MysqlColumn {
        MysqlColumn {
            name: name.into(),
            data_type: data_type.into(),
            column_type: column_type.into(),
            nullable,
            decimal_precision: None,
            decimal_scale: None,
        }
    }

    #[test]
    fn tinyint_one_maps_to_boolean() {
        let c = col("is_active", "tinyint", "tinyint(1)", false);
        let f = c.arrow_field().unwrap();
        assert_eq!(f.data_type(), &DataType::Boolean);
        assert!(!f.is_nullable());
    }

    #[test]
    fn tinyint_n_maps_to_int8() {
        let c = col("kind", "tinyint", "tinyint(4)", true);
        let f = c.arrow_field().unwrap();
        assert_eq!(f.data_type(), &DataType::Int8);
        assert!(f.is_nullable());
    }

    #[test]
    fn integer_family_maps_to_int_widths() {
        assert_eq!(
            col("a", "smallint", "smallint(6)", false)
                .arrow_field()
                .unwrap()
                .data_type(),
            &DataType::Int16
        );
        assert_eq!(
            col("a", "int", "int(11)", false)
                .arrow_field()
                .unwrap()
                .data_type(),
            &DataType::Int32
        );
        assert_eq!(
            col("a", "bigint", "bigint(20)", false)
                .arrow_field()
                .unwrap()
                .data_type(),
            &DataType::Int64
        );
    }

    #[test]
    fn decimal_uses_precision_and_scale() {
        let mut c = col("ltv", "decimal", "decimal(10,2)", false);
        c.decimal_precision = Some(10);
        c.decimal_scale = Some(2);
        match c.arrow_field().unwrap().data_type() {
            DataType::Decimal128(p, s) => {
                assert_eq!(*p, 10);
                assert_eq!(*s, 2);
            }
            other => panic!("expected Decimal128(10,2); got {other:?}"),
        }
    }

    #[test]
    fn varchar_and_text_map_to_utf8() {
        for (dt, ct) in &[
            ("varchar", "varchar(255)"),
            ("text", "text"),
            ("longtext", "longtext"),
            ("char", "char(36)"),
            ("enum", "enum('a','b')"),
            ("json", "json"),
        ] {
            assert_eq!(
                col("c", dt, ct, false).arrow_field().unwrap().data_type(),
                &DataType::Utf8,
                "{dt} should map to Utf8"
            );
        }
    }

    #[test]
    fn binary_family_maps_to_large_binary() {
        for (dt, ct) in &[
            ("binary", "binary(16)"),
            ("varbinary", "varbinary(255)"),
            ("blob", "blob"),
            ("longblob", "longblob"),
        ] {
            assert_eq!(
                col("c", dt, ct, false).arrow_field().unwrap().data_type(),
                &DataType::LargeBinary,
                "{dt} should map to LargeBinary"
            );
        }
    }

    #[test]
    fn datetime_and_timestamp_map_to_micros() {
        for dt in &["datetime", "timestamp"] {
            assert_eq!(
                col("when", dt, dt, true).arrow_field().unwrap().data_type(),
                &DataType::Timestamp(TimeUnit::Microsecond, None)
            );
        }
    }

    #[test]
    fn date_maps_to_date32() {
        assert_eq!(
            col("d", "date", "date", false)
                .arrow_field()
                .unwrap()
                .data_type(),
            &DataType::Date32
        );
    }

    #[test]
    fn unknown_type_errors_actionably() {
        let err = col("c", "geography", "geography", false)
            .arrow_field()
            .unwrap_err();
        match err {
            ConnectorError::Backend(msg) => {
                assert!(msg.contains("unsupported"));
                assert!(msg.contains("geography"));
                assert!(msg.contains("narrow the source access scope") || msg.contains("Narrow"));
            }
            other => panic!("expected Backend error; got {other:?}"),
        }
    }
}
