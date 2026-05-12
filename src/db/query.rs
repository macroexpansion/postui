//! Ad-hoc SQL execution.

use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::{
    db::{PgConn, types::row_to_strings},
    error::DbError,
};

/// One result set returned by a single statement.
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub statement: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub elapsed_ms: u128,
    pub affected: Option<u64>, // for non-SELECT
}

/// Execute one or more statements separated by `;`. Returns one `ResultSet`
/// per statement that returned rows; non-row statements yield empty rows but
/// `affected = Some(n)`.
pub async fn execute(
    conn: &PgConn,
    sql: &str,
    cancel: CancellationToken,
) -> Result<Vec<ResultSet>, DbError> {
    let stmts = split_statements(sql);
    let mut out = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let trimmed = stmt.trim();
        if trimmed.is_empty() {
            continue;
        }
        let started = Instant::now();

        let is_select = trimmed.to_ascii_uppercase().starts_with("SELECT")
            || trimmed.to_ascii_uppercase().starts_with("WITH");

        let res = tokio::select! {
            _ = cancel.cancelled() => return Err(DbError::Cancelled),
            r = async {
                if is_select {
                    let rows = conn.client().query(trimmed, &[]).await
                        .map_err(|e| DbError::Query { sql: trimmed.into(), source: Box::new(e) })?;
                    let headers: Vec<String> = rows.first()
                        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
                        .unwrap_or_default();
                    let body: Vec<Vec<String>> = rows.iter().map(row_to_strings).collect();
                    Ok::<_, DbError>(ResultSet {
                        statement: trimmed.into(),
                        headers,
                        rows: body,
                        elapsed_ms: started.elapsed().as_millis(),
                        affected: None,
                    })
                } else {
                    let n = conn.client().execute(trimmed, &[]).await
                        .map_err(|e| DbError::Query { sql: trimmed.into(), source: Box::new(e) })?;
                    Ok(ResultSet {
                        statement: trimmed.into(),
                        headers: vec![],
                        rows: vec![],
                        elapsed_ms: started.elapsed().as_millis(),
                        affected: Some(n),
                    })
                }
            } => r?,
        };
        out.push(res);
    }

    Ok(out)
}

/// Split on `;` outside of single-quoted strings and `$$`-delimited blocks.
/// Naïve but sufficient for v1.
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_dollar = false;
    while i < chars.len() {
        let c = chars[i];
        if !in_single && !in_dollar && c == ';' {
            out.push(std::mem::take(&mut buf));
            i += 1;
            continue;
        }
        if !in_dollar && c == '\'' {
            in_single = !in_single;
        }
        if !in_single && i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '$' {
            in_dollar = !in_dollar;
            buf.push('$');
            buf.push('$');
            i += 2;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic_statements() {
        let s = split_statements("SELECT 1; SELECT 2;");
        assert_eq!(s.len(), 2);
        assert!(s[0].trim().starts_with("SELECT 1"));
        assert!(s[1].trim().starts_with("SELECT 2"));
    }

    #[test]
    fn split_preserves_semicolon_inside_string() {
        let s = split_statements("SELECT 'a;b'; SELECT 1;");
        assert_eq!(s.len(), 2);
        assert!(s[0].contains("'a;b'"));
    }

    #[test]
    fn split_preserves_semicolon_inside_dollar_block() {
        let s = split_statements("DO $$ BEGIN PERFORM 1; END $$; SELECT 1;");
        assert_eq!(s.len(), 2);
    }
}
