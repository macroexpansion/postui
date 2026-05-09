//! pg_catalog queries returning typed structs. No tokio_postgres::Row escapes
//! this module.

use crate::{db::PgConn, error::DbError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseInfo {
    pub name: String,
    pub owner: String,
    pub encoding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub estimated_rows: i64,
    pub total_bytes: i64,
}

const SQL_LIST_DATABASES: &str = "
    SELECT d.datname,
           pg_get_userbyid(d.datdba) AS owner,
           pg_encoding_to_char(d.encoding) AS encoding
    FROM pg_database d
    WHERE NOT d.datistemplate
    ORDER BY d.datname";

const SQL_LIST_SCHEMAS: &str = "
    SELECT n.nspname,
           pg_get_userbyid(n.nspowner) AS owner
    FROM pg_namespace n
    WHERE n.nspname NOT LIKE 'pg_%'
      AND n.nspname <> 'information_schema'
    ORDER BY n.nspname";

const SQL_LIST_TABLES: &str = "
    SELECT n.nspname AS schema,
           c.relname AS name,
           c.reltuples::bigint AS estimated_rows,
           pg_total_relation_size(c.oid)::bigint AS total_bytes
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relkind IN ('r', 'p')
      AND n.nspname = $1
    ORDER BY c.relname";

pub async fn list_databases(conn: &PgConn) -> Result<Vec<DatabaseInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_DATABASES, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_DATABASES.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| DatabaseInfo {
        name: r.get(0),
        owner: r.get(1),
        encoding: r.get(2),
    }).collect())
}

pub async fn list_schemas(conn: &PgConn) -> Result<Vec<SchemaInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_SCHEMAS, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_SCHEMAS.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| SchemaInfo {
        name: r.get(0),
        owner: r.get(1),
    }).collect())
}

pub async fn list_tables(conn: &PgConn, schema: &str) -> Result<Vec<TableInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_TABLES, &[&schema])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_TABLES.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| TableInfo {
        schema: r.get(0),
        name: r.get(1),
        estimated_rows: r.get(2),
        total_bytes: r.get(3),
    }).collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexInfo {
    pub name: String,
    pub definition: String,
    pub size_bytes: i64,
    pub scans: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintInfo {
    pub name: String,
    pub kind: String,        // 'PRIMARY KEY' | 'FOREIGN KEY' | 'UNIQUE' | 'CHECK'
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSize {
    pub total_bytes: i64,
    pub heap_bytes: i64,
    pub indexes_bytes: i64,
    pub toast_bytes: i64,
    pub estimated_rows: i64,
}

const SQL_LIST_COLUMNS: &str = "
    SELECT a.attname,
           pg_catalog.format_type(a.atttypid, a.atttypmod),
           NOT a.attnotnull,
           pg_get_expr(d.adbin, d.adrelid),
           col_description(a.attrelid, a.attnum)
    FROM pg_attribute a
    JOIN pg_class c ON c.oid = a.attrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    WHERE n.nspname = $1
      AND c.relname = $2
      AND a.attnum > 0
      AND NOT a.attisdropped
    ORDER BY a.attnum";

const SQL_LIST_INDEXES: &str = "
    SELECT i.indexrelname,
           pg_get_indexdef(idx.indexrelid),
           pg_relation_size(idx.indexrelid)::bigint,
           COALESCE(s.idx_scan, 0)::bigint
    FROM pg_index idx
    JOIN pg_class ic ON ic.oid = idx.indexrelid
    JOIN pg_class c ON c.oid = idx.indrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    JOIN pg_stat_all_indexes i ON i.indexrelid = idx.indexrelid
    LEFT JOIN pg_stat_user_indexes s ON s.indexrelid = idx.indexrelid
    WHERE n.nspname = $1 AND c.relname = $2
    ORDER BY i.indexrelname";

const SQL_LIST_CONSTRAINTS: &str = "
    SELECT con.conname,
           CASE con.contype
             WHEN 'p' THEN 'PRIMARY KEY'
             WHEN 'f' THEN 'FOREIGN KEY'
             WHEN 'u' THEN 'UNIQUE'
             WHEN 'c' THEN 'CHECK'
             WHEN 'x' THEN 'EXCLUSION'
             ELSE 'OTHER'
           END,
           pg_get_constraintdef(con.oid)
    FROM pg_constraint con
    JOIN pg_class c ON c.oid = con.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = $1 AND c.relname = $2
    ORDER BY con.conname";

const SQL_TABLE_SIZE: &str = "
    SELECT pg_total_relation_size(c.oid)::bigint AS total,
           pg_relation_size(c.oid, 'main')::bigint AS heap,
           pg_indexes_size(c.oid)::bigint AS indexes,
           COALESCE(pg_total_relation_size(c.reltoastrelid), 0)::bigint AS toast,
           c.reltuples::bigint AS rows
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = $1 AND c.relname = $2";

pub async fn list_columns(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_COLUMNS, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_COLUMNS.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| ColumnInfo {
        name: r.get(0),
        data_type: r.get(1),
        nullable: r.get(2),
        default: r.get(3),
        comment: r.get(4),
    }).collect())
}

pub async fn list_indexes(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<IndexInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_INDEXES, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_INDEXES.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| IndexInfo {
        name: r.get(0),
        definition: r.get(1),
        size_bytes: r.get(2),
        scans: r.get(3),
    }).collect())
}

pub async fn list_constraints(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<ConstraintInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_CONSTRAINTS, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_CONSTRAINTS.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| ConstraintInfo {
        name: r.get(0),
        kind: r.get(1),
        definition: r.get(2),
    }).collect())
}

pub async fn table_size(conn: &PgConn, schema: &str, table: &str) -> Result<TableSize, DbError> {
    let row = conn.client()
        .query_one(SQL_TABLE_SIZE, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_TABLE_SIZE.into(), source: Box::new(e) })?;
    Ok(TableSize {
        total_bytes: row.get(0),
        heap_bytes: row.get(1),
        indexes_bytes: row.get(2),
        toast_bytes: row.get(3),
        estimated_rows: row.get(4),
    })
}

const SQL_PK: &str = "
    SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod)
    FROM pg_constraint con
    JOIN pg_class c ON c.oid = con.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey)
    WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'p'
    ORDER BY array_position(con.conkey, a.attnum)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkColumn {
    pub name: String,
    pub data_type: String,
}

pub async fn primary_key(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<PkColumn>, DbError> {
    let rows = conn.client()
        .query(SQL_PK, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_PK.into(), source: Box::new(e) })?;
    Ok(rows.into_iter().map(|r| PkColumn {
        name: r.get(0),
        data_type: r.get(1),
    }).collect())
}
