//! Postgres connection wrapper.

pub mod activity;
pub mod catalog;
pub mod mutate;
pub mod query;
pub mod rows;
pub mod types;

use std::sync::Arc;

use tokio_postgres::{Client, NoTls};

use crate::error::DbError;

/// A live async Postgres connection. Cheap to clone (Arc inside).
#[derive(Clone, Debug)]
pub struct PgConn {
    inner: Arc<Client>,
    pub label: String,
}

impl PgConn {
    /// Connect using a libpq-compatible connection string. Spawns the
    /// connection driver task on the current tokio runtime.
    pub async fn connect(target: &str, label: String) -> Result<Self, DbError> {
        let (client, connection) = tokio_postgres::connect(target, NoTls)
            .await
            .map_err(|e| DbError::Connect(e.to_string()))?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(?e, "postgres connection driver exited");
            }
        });

        Ok(Self {
            inner: Arc::new(client),
            label,
        })
    }

    pub fn client(&self) -> &Client {
        &self.inner
    }
}
