use super::RUST_TRIM_WHITESPACE;

#[test]
fn startup_perf_sql_trim_set_is_exactly_rust_whitespace() {
    let mismatched: Vec<u32> = (char::MIN..=char::MAX)
        .filter(|c| c.is_whitespace() != RUST_TRIM_WHITESPACE.contains(*c))
        .map(u32::from)
        .collect();

    assert!(
        mismatched.is_empty(),
        "differs from str::trim: {mismatched:x?}"
    );
}

#[test]
fn startup_perf_sql_trim_blanks_exactly_what_rust_trim_blanks() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    // Neighbours share UTF-8 lead bytes with whitespace chars and must survive.
    let neighbours = [
        "\u{84}", "\u{a1}", "\u{1681}", "\u{200b}", "\u{2030}", "\u{3001}", "x",
    ];
    let values = RUST_TRIM_WHITESPACE
        .chars()
        .map(String::from)
        .chain([String::new(), RUST_TRIM_WHITESPACE.to_string()])
        .chain(neighbours.iter().flat_map(|neighbour| {
            [
                neighbour.to_string(),
                format!("\u{3000}{neighbour}\u{a0}"),
                format!(" \t{neighbour}\u{2028}"),
            ]
        }));

    for value in values {
        let sql_blank: bool = connection
            .query_row(
                "SELECT trim(?1, ?2) = ''",
                [value.as_str(), RUST_TRIM_WHITESPACE],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(sql_blank, value.trim().is_empty(), "{value:?}");
    }
}
