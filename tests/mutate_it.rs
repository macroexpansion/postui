mod common;

use postui::db::{
    catalog::primary_key,
    mutate::{ColumnEdit, LiteralValue, PrimaryKey, build_update},
};

#[tokio::test]
#[ignore = "requires docker"]
async fn build_update_executes_against_real_pg() {
    let db = common::start().await;
    db.conn
        .client()
        .execute("CREATE TABLE public.u (id int PRIMARY KEY, name text)", &[])
        .await
        .unwrap();
    db.conn
        .client()
        .execute("INSERT INTO public.u VALUES (1, 'before')", &[])
        .await
        .unwrap();

    let pk = primary_key(&db.conn, "public", "u").await.unwrap();
    assert_eq!(pk.len(), 1);
    let pk = PrimaryKey {
        columns: vec![("id".into(), LiteralValue::Number("1".into()))],
    };
    let edits = vec![ColumnEdit {
        name: "name".into(),
        value: LiteralValue::Text("after".into()),
    }];
    let sql = build_update("public", "u", &edits, &pk).unwrap();

    let n = db.conn.client().execute(sql.as_str(), &[]).await.unwrap();
    assert_eq!(n, 1);

    let row = db
        .conn
        .client()
        .query_one("SELECT name FROM public.u WHERE id=1", &[])
        .await
        .unwrap();
    let name: String = row.get(0);
    assert_eq!(name, "after");
}
