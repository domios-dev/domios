use std::fmt;

pub const WORK_VALUES: [&str; 6] = ["frontend", "backend", "design", "qa", "infra", "docs"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkFacetViolation {
    Missing,
    Multiple(Vec<String>),
    Invalid(String),
    Malformed(String),
}

impl fmt::Display for WorkFacetViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "missing work tag")?,
            Self::Multiple(tags) => write!(f, "multiple work tags: {tags:?}")?,
            Self::Invalid(tag) => write!(f, "invalid work value: {tag:?}")?,
            Self::Malformed(tag) => write!(f, "malformed work tag: {tag:?}")?,
        }
        write!(
            f,
            "; a plan declaring work requires exactly one work:<value> tag; allowed: {}",
            WORK_VALUES.join(", ")
        )
    }
}

pub fn validate_work_facet<'a>(
    facet_keys: impl IntoIterator<Item = &'a str>,
    tags: impl IntoIterator<Item = &'a str>,
) -> Result<(), WorkFacetViolation> {
    if !facet_keys.into_iter().any(|key| key == "work") {
        return Ok(());
    }
    let work_tags: Vec<&str> = tags
        .into_iter()
        .filter(|tag| {
            tag.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "work")
        })
        .collect();
    let tag = match work_tags.as_slice() {
        [] => return Err(WorkFacetViolation::Missing),
        [tag] => *tag,
        _ => {
            return Err(WorkFacetViolation::Multiple(
                work_tags.into_iter().map(str::to_string).collect(),
            ));
        }
    };
    let Some(value) = tag.strip_prefix("work:") else {
        return Err(WorkFacetViolation::Malformed(tag.into()));
    };
    if value.is_empty() || value.contains(':') || value.chars().any(char::is_whitespace) {
        return Err(WorkFacetViolation::Malformed(tag.into()));
    }
    if !WORK_VALUES.contains(&value) {
        return Err(WorkFacetViolation::Invalid(tag.into()));
    }
    Ok(())
}
