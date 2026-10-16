use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEpoch {
    pub source_session_id: String,
    pub session_binding_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResumeCursor {
    Bytes {
        epoch: ChatEpoch,
        incarnation: String,
        offset: u64,
    },
    Revision {
        epoch: ChatEpoch,
        token: String,
    },
    Items {
        epoch: ChatEpoch,
        count: u64,
        journal_seq: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeSourceKind {
    Bytes,
    Revision,
    Items,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeResetReason {
    EpochChanged,
    Compacted,
    InvalidCursor,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResumePlan {
    Empty,
    Read { from: u64, through: u64 },
    Reset(ResumeResetReason),
    Unsupported(ResumeSourceKind),
}

impl ResumeCursor {
    pub fn source_kind(&self) -> ResumeSourceKind {
        match self {
            Self::Bytes { .. } => ResumeSourceKind::Bytes,
            Self::Revision { .. } => ResumeSourceKind::Revision,
            Self::Items { .. } => ResumeSourceKind::Items,
        }
    }

    pub fn decode(value: &str) -> Result<Self, ResumeResetReason> {
        if value.len() > 4096 {
            return Err(ResumeResetReason::InvalidCursor);
        }
        serde_json::from_str(value).map_err(|_| ResumeResetReason::InvalidCursor)
    }

    pub fn encode(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

pub fn plan_resume(
    cursor: &ResumeCursor,
    epoch: &ChatEpoch,
    incarnation: &str,
    length: u64,
    record_boundary: bool,
    page_bytes: u64,
) -> ResumePlan {
    if let Err(source) = validate_source(cursor, ResumeSourceKind::Bytes) {
        return ResumePlan::Unsupported(source);
    }
    let ResumeCursor::Bytes {
        epoch: expected_epoch,
        incarnation: expected_incarnation,
        offset,
    } = cursor
    else {
        return ResumePlan::Unsupported(ResumeSourceKind::Bytes);
    };
    if expected_epoch != epoch {
        return ResumePlan::Reset(ResumeResetReason::EpochChanged);
    }
    if expected_incarnation != incarnation {
        return ResumePlan::Reset(ResumeResetReason::Compacted);
    }
    if *offset > length || !record_boundary || page_bytes == 0 {
        return ResumePlan::Reset(ResumeResetReason::InvalidCursor);
    }
    if *offset == length {
        return ResumePlan::Empty;
    }
    ResumePlan::Read {
        from: *offset,
        through: offset.saturating_add(page_bytes).min(length),
    }
}

pub fn validate_source(
    cursor: &ResumeCursor,
    source: ResumeSourceKind,
) -> Result<(), ResumeSourceKind> {
    match (source, cursor) {
        (ResumeSourceKind::Bytes, ResumeCursor::Bytes { .. }) => Ok(()),
        _ => Err(source),
    }
}

#[cfg(test)]
mod tests;
