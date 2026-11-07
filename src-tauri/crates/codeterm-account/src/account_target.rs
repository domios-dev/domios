use serde::{Deserialize, Serialize};

/// A saved sign-in issued by another account target than the one compiled into this build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetMismatch {
    pub saved: Option<String>,
    pub build: String,
    pub reason: String,
}

impl TargetMismatch {
    pub fn detect(saved: Option<&str>, build: &str) -> Option<Self> {
        let saved = saved.map(str::trim).filter(|saved| !saved.is_empty())?;
        (saved != build).then(|| Self::new(Some(saved), build))
    }

    pub fn refused(build: &str) -> Self {
        Self::new(None, build)
    }

    /// The refresh refusal cannot name the saved project; a recorded one makes the reason exact.
    pub fn with_saved(self, saved: Option<&str>) -> Self {
        match saved.map(str::trim).filter(|saved| !saved.is_empty()) {
            Some(saved) if self.saved.is_none() && saved != self.build => {
                Self::new(Some(saved), &self.build)
            }
            _ => self,
        }
    }

    fn new(saved: Option<&str>, build: &str) -> Self {
        let owner = saved.unwrap_or("another account target");
        Self {
            saved: saved.map(str::to_owned),
            build: build.to_owned(),
            reason: format!(
                "Saved sign-in belongs to {owner}; this build uses {build}; sign in again"
            ),
        }
    }
}

pub fn build_target() -> &'static str {
    crate::firebase_endpoints::current().project
}

#[cfg(test)]
mod tests;
