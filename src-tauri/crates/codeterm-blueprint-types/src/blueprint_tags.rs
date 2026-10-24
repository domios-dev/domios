use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, serde::Deserialize, Serialize)]
pub struct Tag {
    pub name: String,
    pub description: String,
}

pub fn vocabulary() -> &'static [Tag] {
    static TAGS: OnceLock<Vec<Tag>> = OnceLock::new();
    TAGS.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/blueprint-tags.json"))
            .expect("embedded Blueprint tag vocabulary is valid JSON")
    })
}

pub fn contains(tag: &str) -> bool {
    vocabulary().iter().any(|entry| entry.name == tag)
}

#[derive(Debug, Serialize)]
pub struct TagError {
    pub code: &'static str,
    pub message: String,
    pub suggestions: Vec<String>,
    pub valid_tags: Vec<String>,
}

impl std::fmt::Display for TagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if !self.suggestions.is_empty() {
            write!(f, "; closest valid tags: {}", self.suggestions.join(", "))?;
        }
        Ok(())
    }
}

pub fn validate(tags: &[String]) -> Result<Vec<String>, TagError> {
    let error = |message, suggestions| TagError {
        code: "blueprint_tags_invalid",
        message,
        suggestions,
        valid_tags: vocabulary().iter().map(|tag| tag.name.clone()).collect(),
    };
    if !(1..=3).contains(&tags.len()) {
        return Err(error("Choose 1-3 tags from blueprint tags".into(), vec![]));
    }
    for (index, tag) in tags.iter().enumerate() {
        if !contains(tag) {
            let mut nearest = vocabulary().iter().collect::<Vec<_>>();
            nearest.sort_by_key(|entry| (distance(tag, &entry.name), &entry.name));
            return Err(error(
                format!("Unknown tag {tag:?}"),
                nearest
                    .iter()
                    .take(3)
                    .map(|entry| entry.name.clone())
                    .collect(),
            ));
        }
        if tags[..index].contains(tag) {
            return Err(error(format!("Duplicate tag {tag:?}"), vec![]));
        }
    }
    Ok(tags.to_vec())
}

fn distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut row = (0..=right.len()).collect::<Vec<_>>();
    for (i, a) in left.chars().take(128).enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let prior = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(prior + 1)
                .min(diagonal + usize::from(a != *b));
            diagonal = prior;
        }
    }
    row[right.len()]
}

#[cfg(test)]
mod tests;
