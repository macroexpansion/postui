//! Shared test harness: spin up a Postgres container and produce a PgConn.

use postui::db::PgConn;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

pub struct TestDb {
    pub conn: PgConn,
    _container: ContainerAsync<Postgres>,
}

pub async fn start() -> TestDb {
    let container = Postgres::default()
        .start()
        .await
        .expect("postgres container start");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let conn_str = format!("host={host} port={port} user=postgres password=postgres dbname=postgres");
    let conn = PgConn::connect(&conn_str, "test".into())
        .await
        .expect("connect");
    TestDb { conn, _container: container }
}
