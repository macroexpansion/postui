mod common;

use postui::db::catalog;

#[tokio::test]
#[ignore = "requires docker"]
async fn list_databases_includes_postgres() {
    let db = common::start().await;
    let dbs = catalog::list_databases(&db.conn)
        .await
        .expect("list_databases");
    assert!(dbs.iter().any(|d| d.name == "postgres"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_schemas_includes_public() {
    let db = common::start().await;
    let schemas = catalog::list_schemas(&db.conn).await.expect("list_schemas");
    assert!(schemas.iter().any(|s| s.name == "public"));
    assert!(!schemas.iter().any(|s| s.name.starts_with("pg_")));
    assert!(!schemas.iter().any(|s| s.name == "information_schema"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_tables_returns_created_table() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.t1 (id int)", &[])
        .await
        .unwrap();
    let tables = catalog::list_tables(&db.conn, "public")
        .await
        .expect("list_tables");
    assert!(tables.iter().any(|t| t.name == "t1"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_columns_returns_typed_columns() {
    let db = common::start().await;
    db.conn
        .client()
        .execute(
            "CREATE TABLE public.t (id int NOT NULL, name text DEFAULT 'x', extra jsonb)",
            &[],
        )
        .await
        .unwrap();
    let cols = catalog::list_columns(&db.conn, "public", "t")
        .await
        .unwrap();
    assert_eq!(cols.len(), 3);
    assert_eq!(cols[0].name, "id");
    assert!(!cols[0].nullable);
    assert_eq!(cols[1].default.as_deref(), Some("'x'::text"));
    assert_eq!(cols[2].data_type, "jsonb");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_indexes_returns_pk() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.t (id int PRIMARY KEY)", &[])
        .await
        .unwrap();
    let ix = catalog::list_indexes(&db.conn, "public", "t")
        .await
        .unwrap();
    assert!(ix.iter().any(|i| i.name.contains("pkey")));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_constraints_returns_pk() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.t (id int PRIMARY KEY)", &[])
        .await
        .unwrap();
    let con = catalog::list_constraints(&db.conn, "public", "t")
        .await
        .unwrap();
    assert!(con.iter().any(|c| c.kind == "PRIMARY KEY"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn table_size_returns_nonneg() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.t (id int)", &[])
        .await
        .unwrap();
    let sz = catalog::table_size(&db.conn, "public", "t").await.unwrap();
    assert!(sz.total_bytes >= 0);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn primary_key_returns_pk_cols() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.t (id int PRIMARY KEY, name text)", &[])
        .await
        .unwrap();
    let pk = catalog::primary_key(&db.conn, "public", "t").await.unwrap();
    assert_eq!(pk.len(), 1);
    assert_eq!(pk[0].name, "id");
    assert_eq!(pk[0].data_type, "integer");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn primary_key_returns_empty_for_no_pk() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.npk (x int)", &[])
        .await
        .unwrap();
    let pk = catalog::primary_key(&db.conn, "public", "npk")
        .await
        .unwrap();
    assert!(pk.is_empty());
}

#[tokio::test]
#[ignore = "requires docker"]
async fn primary_key_preserves_composite_order() {
    let db = common::start().await;
    // Columns are declared (a, b) so attnum order is a=1, b=2.
    // PK constraint is declared (b, a) so array_position order is b first, a second.
    // A regression that sorted by attnum directly would return (a, b) instead.
    db.conn
        .client()
        .execute(
            "CREATE TABLE public.composite_pk (a int, b int, PRIMARY KEY (b, a))",
            &[],
        )
        .await
        .unwrap();
    let pk = catalog::primary_key(&db.conn, "public", "composite_pk")
        .await
        .unwrap();
    assert_eq!(pk.len(), 2);
    assert_eq!(pk[0].name, "b", "expected declaration order, got: {pk:?}");
    assert_eq!(pk[1].name, "a", "expected declaration order, got: {pk:?}");
}
