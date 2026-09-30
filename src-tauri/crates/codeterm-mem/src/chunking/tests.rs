use super::*;

fn repeat(unit: &str, count: usize) -> String {
    unit.repeat(count)
}

fn assert_well_formed(input: &str, chunks: &[String]) {
    assert!(
        !chunks.is_empty(),
        "no chunks for input of {} chars",
        input.chars().count()
    );
    for chunk in chunks {
        assert!(chunk.chars().count() <= CHUNK_SIZE);
        assert!(input.contains(chunk.as_str()));
    }
    assert!(input.trim_start().starts_with(chunks[0].as_str()));
    assert!(input
        .trim_end()
        .ends_with(chunks[chunks.len() - 1].as_str()));
}

#[test]
fn chunk_text_multibyte_boundaries() {
    let cjk = "漢";
    let emoji = "🦀";
    let family = "👨‍👩‍👧";
    let mut table: Vec<String> = vec![
        repeat(cjk, CHUNK_SIZE),
        repeat(cjk, CHUNK_SIZE + 1),
        repeat(emoji, 3 * CHUNK_SIZE + 7),
        format!("{} {}", repeat(cjk, 700), repeat(cjk, 700)),
        format!("{}. {}", repeat(emoji, 450), repeat(cjk, 900)),
        format!("{}\n\n{}", repeat(emoji, 799), repeat(cjk, 2)),
        repeat(&format!("{family} "), 900),
        repeat("é漢🦀 ", 1000),
        repeat("a", CHUNK_SIZE - 1) + &repeat(emoji, 5),
    ];
    for offset in 0..6 {
        let mut s = repeat("x", CHUNK_SIZE - CHUNK_OVERLAP - offset);
        s.push(' ');
        s.push_str(&repeat(emoji, CHUNK_SIZE + offset));
        s.push_str(". ");
        s.push_str(&repeat(cjk, CHUNK_SIZE / 2 + offset));
        table.push(s);
    }
    for input in &table {
        let chunks = split_chunks(input);
        assert_well_formed(input, &chunks);
    }
}

#[test]
fn chunk_text_covers_the_whole_body_when_boundaries_sit_early() {
    let input = format!("ab {}", repeat("漢", 5 * CHUNK_SIZE));
    let chunks = split_chunks(&input);
    assert_well_formed(&input, &chunks);
    let covered: usize = chunks.iter().map(|c| c.chars().count()).sum();
    assert!(covered >= input.chars().count() - 3);
}

#[test]
fn chunk_count_is_capped_and_keeps_both_ends() {
    let words: Vec<String> = (0..60_000).map(|i| format!("w{i}")).collect();
    let input = words.join(" ");
    let uncapped = split_uncapped(&input);
    assert!(uncapped.len() > MAX_CHUNKS_PER_ITEM);

    let chunks = split_chunks(&input);
    assert_eq!(chunks.len(), MAX_CHUNKS_PER_ITEM);
    assert_eq!(chunks.first(), uncapped.first());
    assert_eq!(chunks.last(), uncapped.last());
}

#[test]
fn short_text_is_a_single_chunk() {
    assert_eq!(split_chunks("hello 🦀"), vec!["hello 🦀".to_string()]);
}
