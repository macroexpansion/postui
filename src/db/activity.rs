//! pg_stat_activity / pg_locks queries.

use chrono::{DateTime, Utc};

use crate::{db::PgConn, error::DbError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityFilter {
    /// Non-self, non-idle (the :queries view).
    ActiveOnly,
    /// Everything except self.
    All,
}

#[derive(Debug, Clone)]
pub struct ActivityRow {
    pub pid: i32,
    pub usename: Option<String>,
    pub datname: Option<String>,
    pub state: Option<String>,
    pub state_change: Option<DateTime<Utc>>,
    pub wait_event: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LockRow {
    pub pid: i32,
    pub mode: Option<String>,
    pub granted: bool,
    pub relation: Option<String>,
    pub query: Option<String>,
}

const SQL_ACTIVITY_ACTIVE: &str = "
    SELECT pid, usename, datname, state, state_change, wait_event, query
    FROM pg_stat_activity
    WHERE pid <> pg_backend_pid()
      AND state IS DISTINCT FROM 'idle'
    ORDER BY query_start NULLS LAST";

const SQL_ACTIVITY_ALL: &str = "
    SELECT pid, usename, datname, state, state_change, wait_event, query
    FROM pg_stat_activity
    WHERE pid <> pg_backend_pid()
    ORDER BY query_start NULLS LAST";

const SQL_LOCKS: &str = "
    SELECT l.pid, l.mode, l.granted, c.relname,
           a.query
    FROM pg_locks l
    LEFT JOIN pg_class c ON c.oid = l.relation
    LEFT JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE l.pid <> pg_backend_pid()
    ORDER BY l.granted, l.pid";

pub async fn activity(conn: &PgConn, filter: ActivityFilter) -> Result<Vec<ActivityRow>, DbError> {
    let sql = match filter {
        ActivityFilter::ActiveOnly => SQL_ACTIVITY_ACTIVE,
        ActivityFilter::All => SQL_ACTIVITY_ALL,
    };
    let rows = conn.client()
        .query(sql, &[])
        .await
        .map_err(|e| DbError::Query { sql: sql.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| ActivityRow {
        pid: r.get(0),
        usename: r.get(1),
        datname: r.get(2),
        state: r.get(3),
        state_change: r.get(4),
        wait_event: r.get(5),
        query: r.get(6),
    }).collect())
}

pub async fn locks(conn: &PgConn) -> Result<Vec<LockRow>, DbError> {
    let rows = conn.client()
        .query(SQL_LOCKS, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LOCKS.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| LockRow {
        pid: r.get(0),
        mode: r.get(1),
        granted: r.get(2),
        relation: r.get(3),
        query: r.get(4),
    }).collect())
}

pub async fn cancel_backend(conn: &PgConn, pid: i32) -> Result<bool, DbError> {
    let row = conn.client()
        .query_one("SELECT pg_cancel_backend($1)", &[&pid])
        .await
        .map_err(|e| DbError::Query { sql: "pg_cancel_backend".into(), source: Box::new(e) })?;
    Ok(row.get(0))
}

pub async fn terminate_backend(conn: &PgConn, pid: i32) -> Result<bool, DbError> {
    let row = conn.client()
        .query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .map_err(|e| DbError::Query { sql: "pg_terminate_backend".into(), source: Box::new(e) })?;
    Ok(row.get(0))
}
