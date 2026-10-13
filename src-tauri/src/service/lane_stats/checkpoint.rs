use super::{safe_ref_component, CaptureFailure, CheckpointResult, LaneCheckpointRefusal};

pub(super) trait Repository {
    fn uses_lfs(&mut self) -> Result<bool, CaptureFailure>;
    fn non_cone_sparse(&mut self) -> Result<bool, CaptureFailure>;
    fn run(&mut self, args: &[&str], isolated_index: bool) -> Result<String, CaptureFailure>;
}

pub(super) fn create(
    repository: &mut impl Repository,
    attempt_id: &str,
    turn: u32,
) -> Result<CheckpointResult, CaptureFailure> {
    if !safe_ref_component(attempt_id) {
        return Ok(CheckpointResult::Refused(LaneCheckpointRefusal::GitFailure));
    }
    if repository.uses_lfs()? {
        return Ok(CheckpointResult::Refused(
            LaneCheckpointRefusal::LfsRepository,
        ));
    }
    if repository.non_cone_sparse()? {
        return Ok(CheckpointResult::Refused(
            LaneCheckpointRefusal::NonConeSparseCheckout,
        ));
    }
    repository.run(&["read-tree", "HEAD"], true)?;
    repository.run(&["add", "-A", "--", "."], true)?;
    let tree = repository.run(&["write-tree"], true)?;
    let reference = format!("refs/domios/checkpoints/{attempt_id}/{turn}");
    let commit = repository.run(
        &[
            "-c",
            "user.name=CodeTerm",
            "-c",
            "user.email=codeterm@localhost",
            "commit-tree",
            tree.trim(),
            "-p",
            "HEAD",
            "-m",
            "codeterm lane checkpoint",
        ],
        false,
    )?;
    let commit = commit.trim();
    if commit.is_empty() {
        return Err(CaptureFailure::Git(
            "git commit-tree returned no object id".into(),
        ));
    }
    repository.run(&["update-ref", &reference, commit], false)?;
    retain(repository, attempt_id)?;
    Ok(CheckpointResult::Created(reference))
}

pub(super) fn retain(
    repository: &mut impl Repository,
    attempt_id: &str,
) -> Result<(), CaptureFailure> {
    let prefix = format!("refs/domios/checkpoints/{attempt_id}");
    let refs = repository.run(&["for-each-ref", "--format=%(refname)", &prefix], false)?;
    for reference in expired_refs(&prefix, &refs) {
        repository.run(&["update-ref", "-d", reference], false)?;
    }
    Ok(())
}

pub(super) fn expired_refs<'a>(prefix: &str, refs: &'a str) -> Vec<&'a str> {
    let mut refs: Vec<_> = refs
        .lines()
        .filter_map(|reference| {
            super::checkpoint_turn(prefix, reference).map(|turn| (turn, reference))
        })
        .collect();
    refs.sort_by_key(|(turn, _)| *turn);
    let remove_count = refs.len().saturating_sub(super::CHECKPOINTS_PER_ATTEMPT);
    refs.into_iter()
        .take(remove_count)
        .map(|(_, reference)| reference)
        .collect()
}
