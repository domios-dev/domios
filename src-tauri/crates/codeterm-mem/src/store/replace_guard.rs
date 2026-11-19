use super::MemKind;

/// Write-area policy a guarded save enforces atomically inside the backend's save transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceGuard {
    pub allowed_areas: Vec<String>,
    allow_skills: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceGuardError {
    AreaNotAllowed,
    TitleConflict,
}

impl ReplaceGuard {
    pub fn new(allowed_areas: Vec<String>) -> Self {
        Self {
            allowed_areas,
            allow_skills: false,
        }
    }

    pub fn with_skill_saves(mut self, allow_skills: bool) -> Self {
        self.allow_skills = allow_skills;
        self
    }

    pub(crate) fn allows_skill_save(&self, area: &str) -> bool {
        self.allow_skills && self.allows(area)
    }

    pub fn check_insert(&self, area: &str) -> Result<(), ReplaceGuardError> {
        if self.allows(area) {
            Ok(())
        } else {
            Err(ReplaceGuardError::AreaNotAllowed)
        }
    }

    pub fn check_replace(
        &self,
        existing_area: &str,
        existing_kind: MemKind,
        existing_pinned: bool,
    ) -> Result<(), ReplaceGuardError> {
        let replaceable = (matches!(existing_kind, MemKind::Fact | MemKind::Note)
            || self.allow_skills && existing_kind == MemKind::Skill)
            && !existing_pinned;
        if replaceable && self.allows(existing_area) {
            Ok(())
        } else {
            Err(ReplaceGuardError::TitleConflict)
        }
    }

    fn allows(&self, area: &str) -> bool {
        self.allowed_areas.iter().any(|allowed| allowed == area)
    }
}
