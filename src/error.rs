//! Top-level error types.

use std::io;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("db error: {0}")]
    Db(#[from] DbError),
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config IO error: {0}")]
    Io(#[source] io::Error),

    #[error("failed to parse config: {0}")]
    Parse(String),

    #[error("missing env var: {var}")]
    MissingEnv { var: String },

    #[error("invalid postgres uri: {0}")]
    BadUri(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connect failed: {0}")]
    Connect(String),

    #[error("query failed: {source}\n  sql: {sql}")]
    Query {
        sql: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("type conversion error: {0}")]
    Type(String),

    #[error("query cancelled")]
    Cancelled,
}
