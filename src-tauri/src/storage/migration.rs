//! The production legacy codecs behind the Storage importer. They decode captured bytes
//! only: no file, scheduler, sender or agent is reachable from here.

use crate::cron::store::{decode_legacy_cron_file, LegacyCronError};
use crate::sentinel::storage::legacy_import;
use crate::storage::{
    DecodeFailure, DecodeFailureKind, DomainImport, ImportDomain, ImportOutcome, ImportRequest,
    LegacyDecoder, MigrationError, Storage,
};

/// Decoders of every legacy domain this build imports.
pub(crate) struct LegacyCodecs;

impl LegacyDecoder for LegacyCodecs {
    fn decode(&self, domain: ImportDomain, bytes: &[u8]) -> Result<DomainImport, DecodeFailure> {
        json_shape(bytes)?;
        match domain {
            ImportDomain::Cron => decode_legacy_cron_file(bytes)
                .map(DomainImport::Cron)
                .map_err(cron_failure),
            ImportDomain::Sentinel => {
                let text = std::str::from_utf8(bytes).map_err(|error| {
                    DecodeFailure::new(DecodeFailureKind::Malformed, error.to_string())
                })?;
                legacy_import(text)
                    .map(DomainImport::Sentinel)
                    .map_err(|detail| DecodeFailure::new(DecodeFailureKind::Malformed, detail))
            }
            _ => Err(DecodeFailure::new(
                DecodeFailureKind::Unsupported,
                "no legacy decoder for this domain",
            )),
        }
    }
}

/// Bytes that end inside a value are a partial write, not a malformed file.
fn json_shape(bytes: &[u8]) -> Result<(), DecodeFailure> {
    match serde_json::from_slice::<serde::de::IgnoredAny>(bytes) {
        Ok(_) => Ok(()),
        Err(error) if error.is_eof() => Err(DecodeFailure::new(
            DecodeFailureKind::Partial,
            error.to_string(),
        )),
        Err(error) => Err(DecodeFailure::new(
            DecodeFailureKind::Malformed,
            error.to_string(),
        )),
    }
}

fn cron_failure(error: LegacyCronError) -> DecodeFailure {
    let kind = match &error {
        LegacyCronError::Unreadable(_) | LegacyCronError::Corrupt(_) => {
            DecodeFailureKind::Malformed
        }
        LegacyCronError::Unsupported(_) => DecodeFailureKind::Unsupported,
        LegacyCronError::Conflict(_) => DecodeFailureKind::Conflict,
    };
    DecodeFailure::new(kind, error.to_string())
}

/// Imports an inert copy of an instance root. Blocking: call it off the async runtime.
pub(crate) fn import_copied_root(
    storage: &Storage,
    request: &ImportRequest,
) -> Result<ImportOutcome, MigrationError> {
    storage.import_legacy_root(request, &LegacyCodecs)
}
