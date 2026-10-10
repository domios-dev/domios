use super::*;

#[test]
fn structural_names_are_conservative() {
    for (a, b) in [
        ("dev", "Dev"),
        ("dev", "dev_"),
        ("test", "tests"),
        ("code-term", "code_term"),
        ("Code Term", "codeterm"),
        ("deploy", "deploi"),
    ] {
        assert!(similar_names(a, b), "{a} / {b}");
        assert!(similar_names(b, a));
    }
    for (a, b) in [
        ("_dreams", "dreams"),
        ("_graph", "graph"),
        ("deploy", "dev"),
        ("cat", "car"),
        ("dev", "development"),
        ("codeterm", "codeterm-planning"),
        ("deploy", "deployed"),
        ("-", "_"),
    ] {
        assert!(!similar_names(a, b), "{a} / {b}");
    }
}

#[test]
fn candidates_include_counts_samples_and_smaller_first() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("memory.db")).unwrap();
    crate::store::init_schema(&conn).unwrap();
    for (i, area) in [
        "dev", "Dev", "dev_", "dev", "test", "tests", "_dreams", "_graph", "deploy",
    ]
    .iter()
    .enumerate()
    {
        conn.execute("INSERT INTO mem_items (id, uid, kind, scope, title, area, tags_json, importance, created_at, updated_at) VALUES (?1, ?1, 'fact', 'global', ?2, ?3, '[]', 3, 'now', 'now')",
            params![format!("fact_{i}"), format!("Sample {i}"), area]).unwrap();
    }
    let areas = read_areas(&conn).unwrap();
    assert!(areas.iter().all(|a| !a.name.starts_with('_')));
    let dev = areas.iter().find(|a| a.name == "dev").unwrap();
    assert_eq!(dev.count, 2);
    assert_eq!(dev.samples, vec!["Sample 0", "Sample 3"]);
    let pairs = candidate_pairs(&areas);
    assert_eq!(pairs.len(), 4);
    assert!(pairs.iter().all(|(a, b)| a.count <= b.count));
    assert!(pairs
        .iter()
        .any(|(a, b)| a.name == "Dev" && b.name == "dev"));
    assert!(pairs
        .iter()
        .all(|(a, b)| a.name != "deploy" && b.name != "deploy"));
    let output = render_candidates(&conn).unwrap().unwrap();
    for sample in [&dev.samples[0], &dev.samples[1]] {
        assert!(output.contains(sample));
    }
}
