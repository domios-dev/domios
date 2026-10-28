use super::*;

#[test]
fn sqlite_recall_temporal() {
    let (_dir, store) = fixture();
    temporal(&store);
}

#[test]
fn sqlite_recall_lexical_lift() {
    let (_dir, store) = fixture();
    lexical_lift(&store);
}

#[test]
fn sqlite_recall_graph() {
    let (_dir, store) = fixture();
    graph(&store);
}

#[test]
fn sqlite_recall_vector_refinements() {
    let (_dir, store) = fixture();
    vector_refinements(&store);
}
