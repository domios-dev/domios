use std::collections::HashSet;

use rand::seq::SliceRandom;

const ADJECTIVES: &[&str] = &[
    "admiring",
    "adoring",
    "agile",
    "amazing",
    "ancient",
    "bold",
    "brave",
    "bright",
    "calm",
    "charming",
    "cheerful",
    "clever",
    "cosmic",
    "crafty",
    "crisp",
    "curious",
    "dapper",
    "daring",
    "dazzling",
    "deft",
    "delightful",
    "dreamy",
    "eager",
    "eclectic",
    "elegant",
    "electric",
    "eloquent",
    "ethereal",
    "exuberant",
    "fancy",
    "fearless",
    "festive",
    "fierce",
    "focused",
    "friendly",
    "frosty",
    "gallant",
    "gentle",
    "gifted",
    "glowing",
    "golden",
    "graceful",
    "grand",
    "happy",
    "hardy",
    "heroic",
    "honest",
    "hopeful",
    "humble",
    "inspired",
    "jolly",
    "jovial",
    "jubilant",
    "keen",
    "kind",
    "lively",
    "lucid",
    "lucky",
    "luminous",
    "magical",
    "majestic",
    "mellow",
    "merry",
    "mighty",
    "mindful",
    "modest",
    "mystic",
    "noble",
    "nostalgic",
    "optimistic",
    "peaceful",
    "plucky",
    "polite",
    "proud",
    "quick",
    "quiet",
    "quirky",
    "radiant",
    "regal",
    "robust",
    "sage",
    "savvy",
    "serene",
    "sharp",
    "shiny",
    "silent",
    "silly",
    "sleek",
    "snappy",
    "sparkling",
    "spirited",
    "steady",
    "stellar",
    "sturdy",
    "sublime",
    "sunny",
    "swift",
    "tender",
    "thoughtful",
    "thriving",
    "tidy",
    "tranquil",
    "trusty",
    "valiant",
    "vibrant",
    "vigilant",
    "vivid",
    "warm",
    "whimsical",
    "wise",
    "witty",
    "zany",
    "zealous",
    "zen",
];

const NOTABLES: &[&str] = &[
    "newton", "curie", "tesla", "bohr", "darwin", "edison", "kelvin", "watt", "volta", "pascal",
    "halley", "hooke", "boyle", "joule", "ampere", "ohm", "fermi", "bell", "morse", "nobel",
    "planck", "turing", "lovelace", "hopper", "ritchie", "pasteur", "faraday", "mendel", "galileo",
    "kepler", "hertz", "watson", "franklin", "sagan", "hubble", "tycho", "euler", "gauss",
    "fermat", "monet", "dali", "goya", "klimt", "vermeer", "turner", "renoir", "manet", "degas",
    "munch", "klee", "miro", "rodin", "rubens", "raphael", "titian", "bach", "mozart", "verdi",
    "liszt", "chopin", "brahms", "haydn", "grieg", "bizet", "ravel", "holst", "elgar", "handel",
    "vivaldi", "puccini", "twain", "poe", "wilde", "dumas", "hugo", "verne", "orwell", "dante",
    "byron", "keats", "frost", "joyce", "camus", "tolkien", "austen", "dickens", "kafka", "ibsen",
    "cook", "drake", "polo", "hudson", "cabot", "scott", "amundsen", "columbus", "ford", "diesel",
    "wright", "fulton",
];

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

/// Generate a single-word Title Case mnemonic name like "Curie" or "Monet".
/// Short, easy-to-say surnames across science/art/music/letters/exploration.
/// Adjectives stay around as a fallback when the notables list is exhausted.
pub fn generate_tab_name() -> String {
    let mut rng = rand::thread_rng();
    let name = NOTABLES
        .choose(&mut rng)
        .copied()
        .or_else(|| ADJECTIVES.choose(&mut rng).copied())
        .unwrap_or("Tab");
    capitalize(name)
}

pub fn generate_unique_tab_name<'a>(existing: impl IntoIterator<Item = &'a str>) -> String {
    let occupied: HashSet<&str> = existing.into_iter().collect();
    let pool_size = NOTABLES.len() + ADJECTIVES.len();

    for _ in 0..pool_size.saturating_mul(2).max(1) {
        let candidate = generate_tab_name();
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }

    for name in NOTABLES.iter().chain(ADJECTIVES) {
        let candidate = capitalize(name);
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }

    let mut index = 2;
    loop {
        let candidate = format!("Tab{index}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_name_is_single_capitalized_word() {
        for _ in 0..20 {
            let name = generate_tab_name();
            assert!(!name.contains(' '), "expected single word: '{}'", name);
            assert!(!name.is_empty());
            let first = name.chars().next().unwrap();
            assert!(first.is_uppercase(), "expected capitalized: '{}'", name);
        }
    }

    #[test]
    fn lists_are_nonempty() {
        assert!(!ADJECTIVES.is_empty());
        assert!(!NOTABLES.is_empty());
    }

    #[test]
    fn unique_name_avoids_existing_tab_titles() {
        let occupied: Vec<String> = NOTABLES
            .iter()
            .take(NOTABLES.len() - 1)
            .map(|name| capitalize(name))
            .collect();
        let available = capitalize(NOTABLES.last().unwrap());

        let generated = generate_unique_tab_name(occupied.iter().map(String::as_str));

        assert_eq!(generated, available);
    }

    #[test]
    fn tab_name_contract_fallback_name_is_one_word() {
        let occupied = NOTABLES
            .iter()
            .chain(ADJECTIVES)
            .map(|name| capitalize(name))
            .collect::<Vec<_>>();
        let generated = generate_unique_tab_name(occupied.iter().map(String::as_str));
        assert!(!generated.chars().any(char::is_whitespace));
    }
}
