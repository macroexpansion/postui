//! Paged row fetch with LIMIT/OFFSET. Returns column headers + display strings.

use crate::{
    db::{PgConn, types::row_to_strings},
    error::DbError,
};

pub const PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone)]
pub struct Page {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub offset: i64,
    pub estimated_total: i64,
}

pub async fn fetch_page(
    conn: &PgConn,
    schema: &str,
    table: &str,
    offset: i64,
) -> Result<Page, DbError> {
    let qualified = format!(
        "\"{}\".\"{}\"",
        schema.replace('"', "\"\""),
        table.replace('"', "\"\"")
    );
    let sql = format!("SELECT * FROM {qualified} LIMIT {PAGE_SIZE} OFFSET {offset}");
    let rows = conn
        .client()
        .query(&sql, &[])
        .await
        .map_err(|e| DbError::Query {
            sql: sql.clone(),
            source: Box::new(e),
        })?;

    let headers: Vec<String> = if let Some(first) = rows.first() {
        first
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect()
    } else {
        let info_sql = format!("SELECT * FROM {qualified} LIMIT 0");
        let r = conn
            .client()
            .query(&info_sql, &[])
            .await
            .map_err(|e| DbError::Query {
                sql: info_sql.clone(),
                source: Box::new(e),
            })?;
        r.first()
            .map(|f| f.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default()
    };

    let body: Vec<Vec<String>> = rows.iter().map(row_to_strings).collect();

    let est_sql = format!(
        "SELECT reltuples::bigint FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = $1 AND c.relname = $2"
    );
    let est: i64 = conn
        .client()
        .query_opt(&est_sql, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query {
            sql: est_sql.clone(),
            source: Box::new(e),
        })?
        .map(|r| r.get(0))
        .unwrap_or(0);

    Ok(Page {
        headers,
        rows: body,
        offset,
        estimated_total: est,
    })
}
