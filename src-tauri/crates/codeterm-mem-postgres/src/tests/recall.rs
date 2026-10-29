use super::*;
use codeterm_mem::backend::contract::recall;

fn parity(contract: fn(&dyn MemStore) -> Vec<Vec<String>>) {
    let (_dir, sqlite) = recall::fixture();
    let postgres = Fixture::new();
    for page in export_all(&sqlite) {
        MemStore::import_page(
            postgres.store(),
            &WriteContext::system("recall-fixture"),
            &page,
        )
        .unwrap();
    }
    assert_eq!(contract(&sqlite), contract(postgres.store()));
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn recall_temporal_parity() {
    parity(recall::temporal);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn recall_lexical_lift_parity() {
    parity(recall::lexical_lift);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn recall_graph_parity() {
    parity(recall::graph);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn recall_vector_refinements_parity() {
    parity(recall::vector_refinements);
}
