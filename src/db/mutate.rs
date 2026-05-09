//! UPDATE / INSERT / DELETE SQL builders. Output is intentionally NOT
//! parameterized for v1 — we render the values inline so the user sees the
//! exact statement that will run in the confirm modal. We escape strings via
//! Postgres' E'' quoting and bytea via \x notation.

use crate::error::DbError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    Null,
    Bool(bool),
    Number(String),    // already-formatted numeric/integer/decimal
    Text(String),
    Bytes(Vec<u8>),
    /// Raw SQL fragment, used only for things like `now()` — must be safe.
    Raw(String),
}

#[derive(Debug, Clone)]
pub struct ColumnEdit {
    pub name: String,
    pub value: LiteralValue,
}

#[derive(Debug, Clone)]
pub struct PrimaryKey {
    /// Column name + literal value uniquely identifying a row.
    pub columns: Vec<(String, LiteralValue)>,
}

pub fn build_update(
    schema: &str,
    table: &str,
    edits: &[ColumnEdit],
    pk: &PrimaryKey,
) -> Result<String, DbError> {
    if edits.is_empty() {
        return Err(DbError::Type("no columns to update".into()));
    }
    if pk.columns.is_empty() {
        return Err(DbError::Type("table has no primary key — refusing UPDATE".into()));
    }
    let set = edits.iter().map(|e| format!("{} = {}", quote_ident(&e.name), render(&e.value)))
        .collect::<Vec<_>>().join(", ");
    let where_ = pk_where(pk);
    Ok(format!("UPDATE {}.{} SET {} WHERE {};",
        quote_ident(schema), quote_ident(table), set, where_))
}

pub fn build_delete(schema: &str, table: &str, pk: &PrimaryKey) -> Result<String, DbError> {
    if pk.columns.is_empty() {
        return Err(DbError::Type("table has no primary key — refusing DELETE".into()));
    }
    Ok(format!("DELETE FROM {}.{} WHERE {};",
        quote_ident(schema), quote_ident(table), pk_where(pk)))
}

pub fn build_insert(
    schema: &str,
    table: &str,
    values: &[ColumnEdit],
) -> Result<String, DbError> {
    if values.is_empty() {
        return Err(DbError::Type("no columns to insert".into()));
    }
    let cols = values.iter().map(|e| quote_ident(&e.name)).collect::<Vec<_>>().join(", ");
    let vals = values.iter().map(|e| render(&e.value)).collect::<Vec<_>>().join(", ");
    Ok(format!("INSERT INTO {}.{} ({}) VALUES ({});",
        quote_ident(schema), quote_ident(table), cols, vals))
}

fn pk_where(pk: &PrimaryKey) -> String {
    pk.columns.iter()
        .map(|(c, v)| format!("{} = {}", quote_ident(c), render(v)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn render(v: &LiteralValue) -> String {
    match v {
        LiteralValue::Null => "NULL".into(),
        LiteralValue::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        LiteralValue::Number(s) => s.clone(),
        LiteralValue::Text(s) => format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        LiteralValue::Bytes(b) => {
            let mut out = String::from("'\\x");
            for byte in b { out.push_str(&format!("{byte:02x}")); }
            out.push('\'');
            out
        }
        LiteralValue::Raw(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk_int(name: &str, n: i64) -> PrimaryKey {
        PrimaryKey { columns: vec![(name.into(), LiteralValue::Number(n.to_string()))] }
    }

    #[test]
    fn update_simple() {
        let edits = vec![ColumnEdit { name: "email".into(), value: LiteralValue::Text("ada@x.io".into()) }];
        let sql = build_update("public", "users", &edits, &pk_int("id", 1042)).unwrap();
        assert_eq!(sql, "UPDATE \"public\".\"users\" SET \"email\" = E'ada@x.io' WHERE \"id\" = 1042;");
    }

    #[test]
    fn update_with_quote_in_text_is_escaped() {
        let edits = vec![ColumnEdit { name: "name".into(), value: LiteralValue::Text("O'Brien".into()) }];
        let sql = build_update("public", "users", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("E'O\\'Brien'"), "got: {sql}");
    }

    #[test]
    fn update_no_pk_errors() {
        let edits = vec![ColumnEdit { name: "x".into(), value: LiteralValue::Number("1".into()) }];
        let pk = PrimaryKey { columns: vec![] };
        let err = build_update("p", "t", &edits, &pk).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn update_no_edits_errors() {
        let err = build_update("p", "t", &[], &pk_int("id", 1)).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn delete_simple() {
        let sql = build_delete("public", "users", &pk_int("id", 9)).unwrap();
        assert_eq!(sql, "DELETE FROM \"public\".\"users\" WHERE \"id\" = 9;");
    }

    #[test]
    fn delete_no_pk_errors() {
        let err = build_delete("p", "t", &PrimaryKey { columns: vec![] }).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn insert_simple() {
        let values = vec![
            ColumnEdit { name: "id".into(), value: LiteralValue::Number("1".into()) },
            ColumnEdit { name: "email".into(), value: LiteralValue::Text("a@b.c".into()) },
        ];
        let sql = build_insert("public", "users", &values).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"id\", \"email\") VALUES (1, E'a@b.c');"
        );
    }

    #[test]
    fn null_renders_as_null() {
        let edits = vec![ColumnEdit { name: "deleted_at".into(), value: LiteralValue::Null }];
        let sql = build_update("p", "t", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("\"deleted_at\" = NULL"));
    }

    #[test]
    fn bytes_render_as_hex() {
        let edits = vec![ColumnEdit { name: "data".into(), value: LiteralValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef]) }];
        let sql = build_update("p", "t", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("'\\xdeadbeef'"));
    }

    #[test]
    fn composite_pk_uses_and() {
        let pk = PrimaryKey { columns: vec![
            ("a".into(), LiteralValue::Number("1".into())),
            ("b".into(), LiteralValue::Text("x".into())),
        ]};
        let sql = build_delete("p", "t", &pk).unwrap();
        assert!(sql.contains("\"a\" = 1 AND \"b\" = E'x'"));
    }
}
