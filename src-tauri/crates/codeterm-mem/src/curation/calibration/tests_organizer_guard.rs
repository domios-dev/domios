use super::*;

#[test]
fn calibration_promotion_refuses_neighbor_authored_item() {
    let conn = Connection::open_in_memory().unwrap();
    crate::store::init_schema(&conn).unwrap();
    conn.execute("INSERT INTO mem_items(id,uid,kind,scope,title,created_at,updated_at,author_kind,author_uid,author_friendship) VALUES('n','n','fact','global','Neighbor','now','now','neighbor','uid','friendship')", []).unwrap();
    assert!(apply_promote(
        &conn,
        &Candidate {
            id: "n".into(),
            kind: "fact".into(),
            importance: 3.0,
            action: "promote".into()
        },
        4.0
    )
    .is_err());
    let importance: f64 = conn
        .query_row("SELECT importance FROM mem_items WHERE id='n'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(importance, 3.0);
}
