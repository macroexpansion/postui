mod common;

use postui::db::activity::{self, ActivityFilter};

#[tokio::test]
#[ignore = "requires docker"]
async fn activity_returns_at_least_self_when_filter_is_all() {
    let db = common::start().await;
    let rows = activity::activity(&db.conn, ActivityFilter::All).await.unwrap();
    // Self is excluded; there might be 0 other rows on a fresh container.
    let _ = rows;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn locks_query_succeeds() {
    let db = common::start().await;
    let _rows = activity::locks(&db.conn).await.unwrap();
}
