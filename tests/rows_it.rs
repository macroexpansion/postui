mod common;

use postui::db::rows;

#[tokio::test]
#[ignore = "requires docker"]
async fn fetch_page_returns_first_100() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.r (id int, name text)",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.r SELECT g, 'name-'||g FROM generate_series(1, 250) g",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "r", 0).await.unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.offset, 0);
    assert_eq!(page.headers, vec!["id".to_string(), "name".to_string()]);
    assert_eq!(page.rows[0][0], "1");
    assert_eq!(page.rows[0][1], "name-1");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn fetch_page_with_offset() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.r (id int)",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.r SELECT g FROM generate_series(1, 250) g",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "r", 100).await.unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.rows[0][0], "101");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn type_conversions_round_trip() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (
            i  int,
            t  text,
            b  bool,
            ts timestamptz,
            j  jsonb,
            u  uuid,
            n  numeric,
            ba bytea
        )",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.t VALUES (
            1, 'hi', true, '2024-01-01T12:00:00Z',
            '{\"k\":1}'::jsonb,
            '11111111-2222-3333-4444-555555555555'::uuid,
            12.34,
            '\\x010203'::bytea
        )",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "t", 0).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    let r = &page.rows[0];
    assert_eq!(r[0], "1");
    assert_eq!(r[1], "hi");
    assert_eq!(r[2], "true");
    assert!(r[3].starts_with("2024-01-01"));
    assert!(r[4].contains("\"k\""));
    assert!(r[5].starts_with("11111111"));
    assert_eq!(r[6], "12.34");
    assert_eq!(r[7], "\\x010203");
}
