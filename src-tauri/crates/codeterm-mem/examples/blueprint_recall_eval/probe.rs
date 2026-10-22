use crate::confirm::fixture_library;
use codeterm_mem::blueprint_library::index::cosine;

pub fn probe(doc: &str) -> Result<(), String> {
    let library = fixture_library()?;
    let item = library.item_of.get(doc).ok_or("unknown doc")?;
    let row = library.mem.blueprint_index(item)?.ok_or("no index row")?;
    println!("canon: {}", row.canon);
    for intent in library
        .mem
        .list_blueprint_solutions(crate::library::ALL_AREAS)?
        .into_iter()
        .filter(|e| &e.id == item)
        .flat_map(|e| e.intents)
    {
        let passage = codeterm_mem::embeddings::embed(&intent)?;
        let again = codeterm_mem::embeddings::embed(&intent)?;
        let query = codeterm_mem::embeddings::query_embed(&intent)?;
        println!(
            "{intent:?}: passage/again {:.4} passage/query {:.4}",
            cosine(&passage, &again),
            cosine(&passage, &query)
        );
    }
    Ok(())
}
