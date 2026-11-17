use super::*;
use rusqlite::{params, Connection};

#[test]
fn full_skill_saves_preserve_author_guards() {
    let conn = Connection::open_in_memory().unwrap();
    run_steps(&conn, &[baseline, authorship]).unwrap();
    conn.execute("INSERT INTO mem_items(id,uid,kind,scope,title,created_at,updated_at,author_kind,author_uid,author_friendship) VALUES('retained','retained','fact','global','retained','now','now','neighbor','friend','friendship')", []).unwrap();
    run(&conn).unwrap();
    run(&conn).unwrap();
    let retained: String = conn
        .query_row(
            "SELECT title FROM mem_items WHERE id='retained'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, "retained");
    let insert = |id: &str, kind: &str, pinned: bool, authorized: bool| {
        conn.execute(
            "INSERT INTO mem_items(id,uid,kind,scope,title,created_at,updated_at,author_kind,author_uid,author_friendship,is_pinned_wakeup,neighbor_skill_authorized)
             VALUES(?1,?1,?2,'global',?1,'now','now','neighbor','friend','friendship',?3,?4)",
            params![id, kind, pinned, authorized],
        )
    };
    assert!(insert("unmarked", "skill", false, false).is_err());
    insert("skill", "skill", false, true).unwrap();
    conn.execute(
        "UPDATE mem_items SET body_inline='updated' WHERE id='skill'",
        [],
    )
    .unwrap();
    for kind in ["rule", "preference", "secret"] {
        assert!(insert(kind, kind, false, true).is_err(), "{kind}");
        assert!(
            conn.execute("UPDATE mem_items SET kind=?1 WHERE id='skill'", [kind])
                .is_err(),
            "{kind}"
        );
    }
    assert!(insert("pinned", "skill", true, true).is_err());
    assert!(conn
        .execute(
            "UPDATE mem_items SET is_pinned_wakeup=1 WHERE id='skill'",
            []
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE mem_items SET neighbor_skill_authorized=0 WHERE id='skill'",
            []
        )
        .is_err());
    insert("learning", "learning", false, false).unwrap();
    assert!(conn
        .execute("UPDATE mem_items SET kind='fact' WHERE id='learning'", [])
        .is_err());
    insert("fact", "fact", false, false).unwrap();
    assert!(conn
        .execute(
            "UPDATE mem_items SET kind='skill',neighbor_skill_authorized=1 WHERE id='fact'",
            []
        )
        .is_err());
    let skill: (String, String, bool) = conn
        .query_row(
            "SELECT kind,body_inline,is_pinned_wakeup FROM mem_items WHERE id='skill'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(skill, ("skill".into(), "updated".into(), false));
}
