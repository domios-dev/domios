use rand::seq::SliceRandom;

const ADJECTIVES: &[&str] = &[
    "admiring", "adoring", "agile", "amazing", "ancient", "bold", "brave", "bright",
    "calm", "charming", "cheerful", "clever", "cosmic", "crafty", "crisp", "curious",
    "dapper", "daring", "dazzling", "deft", "delightful", "dreamy", "eager", "eclectic",
    "elegant", "electric", "eloquent", "ethereal", "exuberant", "fancy", "fearless",
    "festive", "fierce", "focused", "friendly", "frosty", "gallant", "gentle", "gifted",
    "glowing", "golden", "graceful", "grand", "happy", "hardy", "heroic", "honest",
    "hopeful", "humble", "inspired", "jolly", "jovial", "jubilant", "keen", "kind",
    "lively", "lucid", "lucky", "luminous", "magical", "majestic", "mellow", "merry",
    "mighty", "mindful", "modest", "mystic", "noble", "nostalgic", "optimistic",
    "peaceful", "plucky", "polite", "proud", "quick", "quiet", "quirky", "radiant",
    "regal", "robust", "sage", "savvy", "serene", "sharp", "shiny", "silent", "silly",
    "sleek", "snappy", "sparkling", "spirited", "steady", "stellar", "sturdy", "sublime",
    "sunny", "swift", "tender", "thoughtful", "thriving", "tidy", "tranquil", "trusty",
    "valiant", "vibrant", "vigilant", "vivid", "warm", "whimsical", "wise", "witty",
    "zany", "zealous", "zen",
];

const SCIENTISTS: &[&str] = &[
    "archimedes", "aristotle", "avicenna", "babbage", "bardeen", "bell", "bhabha",
    "bohr", "boltzmann", "born", "boyle", "brahe", "brahmagupta", "cantor", "carnot",
    "cavendish", "chandrasekhar", "copernicus", "curie", "dalton", "darwin", "davinci",
    "dijkstra", "dirac", "edison", "einstein", "euclid", "euler", "faraday", "fermat",
    "fermi", "feynman", "fibonacci", "franklin", "galileo", "galois", "gauss", "godel",
    "hamilton", "hawking", "heisenberg", "herschel", "hertz", "hilbert", "hopper",
    "hubble", "huygens", "jansky", "kelvin", "kepler", "knuth", "lagrange", "lamarr",
    "laplace", "lavoisier", "leibniz", "lemaitre", "liskov", "lorenz", "lovelace",
    "mandelbrot", "marconi", "maxwell", "mccarthy", "mendel", "mendeleev", "minkowski",
    "minsky", "mirzakhani", "mobius", "morse", "nash", "neumann", "newton", "nightingale",
    "noether", "nyquist", "oppenheimer", "pasteur", "pauli", "pavlov", "peano", "penrose",
    "planck", "poincare", "ptolemy", "ramanujan", "ritchie", "rontgen", "russell", "sagan",
    "schrodinger", "shannon", "shockley", "sklodowska", "snell", "sommerfeld", "tesla",
    "thomson", "torvalds", "turing", "vanleeuwenhoek", "volta", "watson", "watt", "wiener",
    "wozniak", "yang", "yonath", "young",
];

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

/// Generate a Title Case mnemonic name like "Nostalgic Curie".
pub fn generate_tab_name() -> String {
    let mut rng = rand::thread_rng();
    let adj = ADJECTIVES.choose(&mut rng).copied().unwrap_or("Nameless");
    let sci = SCIENTISTS.choose(&mut rng).copied().unwrap_or("Tab");
    format!("{} {}", capitalize(adj), capitalize(sci))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_name_is_title_case_with_space() {
        for _ in 0..20 {
            let name = generate_tab_name();
            let parts: Vec<&str> = name.split(' ').collect();
            assert_eq!(parts.len(), 2, "expected two words: '{}'", name);
            for p in &parts {
                assert!(!p.is_empty());
                let first = p.chars().next().unwrap();
                assert!(first.is_uppercase(), "expected capitalized: '{}'", p);
            }
        }
    }

    #[test]
    fn lists_are_nonempty() {
        assert!(!ADJECTIVES.is_empty());
        assert!(!SCIENTISTS.is_empty());
    }
}
