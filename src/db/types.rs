//! Convert tokio_postgres rows into displayable strings.

use std::fmt::Write;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::Value as Json;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

/// Convert a single column value to its display string. Returns "NULL" for
/// nulls, "<unsupported>" for types we don't yet handle.
pub fn col_to_string(row: &Row, idx: usize) -> String {
    let col_type = row.columns()[idx].type_();
    match *col_type {
        Type::BOOL => row.try_get::<_, Option<bool>>(idx).map_or_else(err, opt_str),
        Type::INT2 => row.try_get::<_, Option<i16>>(idx).map_or_else(err, opt_str),
        Type::INT4 => row.try_get::<_, Option<i32>>(idx).map_or_else(err, opt_str),
        Type::INT8 => row.try_get::<_, Option<i64>>(idx).map_or_else(err, opt_str),
        Type::FLOAT4 => row.try_get::<_, Option<f32>>(idx).map_or_else(err, opt_str),
        Type::FLOAT8 => row.try_get::<_, Option<f64>>(idx).map_or_else(err, opt_str),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            row.try_get::<_, Option<String>>(idx).map_or_else(err, |o| o.unwrap_or_else(null))
        }
        Type::UUID => row.try_get::<_, Option<Uuid>>(idx).map_or_else(err, |o| o.map(|u| u.to_string()).unwrap_or_else(null)),
        Type::TIMESTAMPTZ => row.try_get::<_, Option<DateTime<Utc>>>(idx).map_or_else(err, |o| o.map(|t| t.to_rfc3339()).unwrap_or_else(null)),
        Type::TIMESTAMP => row.try_get::<_, Option<NaiveDateTime>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::DATE => row.try_get::<_, Option<NaiveDate>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::TIME => row.try_get::<_, Option<NaiveTime>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::JSON | Type::JSONB => row.try_get::<_, Option<Json>>(idx).map_or_else(err, |o| o.map(|j| j.to_string()).unwrap_or_else(null)),
        Type::NUMERIC => row.try_get::<_, Option<String>>(idx).map_or_else(err, |o| o.unwrap_or_else(null)),
        Type::BYTEA => row.try_get::<_, Option<Vec<u8>>>(idx).map_or_else(err, |o| o.map(hex).unwrap_or_else(null)),
        Type::TEXT_ARRAY | Type::VARCHAR_ARRAY => row.try_get::<_, Option<Vec<String>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", v.join(","))).unwrap_or_else(null)),
        Type::INT4_ARRAY => row.try_get::<_, Option<Vec<i32>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", join_strs(&v))).unwrap_or_else(null)),
        Type::INT8_ARRAY => row.try_get::<_, Option<Vec<i64>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", join_strs(&v))).unwrap_or_else(null)),
        _ => "<unsupported>".into(),
    }
}

fn opt_str<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(null)
}

fn null() -> String { "NULL".into() }

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("<err: {e}>")
}

fn hex(bytes: Vec<u8>) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("\\x");
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn join_strs<T: std::fmt::Display>(xs: &[T]) -> String {
    let mut out = String::new();
    for (i, x) in xs.iter().enumerate() {
        if i > 0 { out.push(','); }
        let _ = write!(out, "{x}");
    }
    out
}

/// Convert an entire row to display strings, in column order.
pub fn row_to_strings(row: &Row) -> Vec<String> {
    (0..row.len()).map(|i| col_to_string(row, i)).collect()
}
