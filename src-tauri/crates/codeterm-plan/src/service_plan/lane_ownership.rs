//! The one answer to "may Domios delete this lane": only a worktree and branch
//! Domios provisioned. Every cleanup path asks here before any git mutation.

use crate::plan::LaneOwnership;
use codeterm_util::worktree_cleanup::lane_slug_under_root;

pub const LANE_BRANCH_PREFIX: &str = "lane/";
pub const FOREIGN_CHECKOUT: &str = "foreign checkout, not managed";

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// `None` when the lane is Domios's to clean; otherwise why it is not managed.
/// A recorded `domios_lane` must still sit in the lane root on a `lane/` branch.
/// An unrecorded lane (written before dispatch recorded ownership) counts only
/// on the exact provisioned fingerprint: `<canonical>-worktrees/<slug>`
/// registered as a linked worktree of `canonical` with `lane/<slug>` checked
/// out. `registration` reports that branch (`None`: not a linked worktree);
/// a requested branch must agree with it.
pub fn unmanaged_lane_reason(
    ownership: Option<LaneOwnership>,
    canonical: &str,
    worktree: Option<&str>,
    branch: Option<&str>,
    registration: impl FnOnce(&str) -> Result<Option<Option<String>>, String>,
) -> Option<String> {
    let worktree = non_empty(worktree);
    let branch = non_empty(branch);
    if worktree.is_none() && branch.is_none() {
        return None;
    }
    if ownership == Some(LaneOwnership::ForeignCheckout) {
        return Some(FOREIGN_CHECKOUT.into());
    }
    let slug_of_branch = match branch {
        Some(branch) => match branch.strip_prefix(LANE_BRANCH_PREFIX) {
            Some(slug) if !slug.is_empty() => Some(slug),
            _ => {
                return Some(format!(
                    "branch {branch} is not a Domios lane branch, not managed"
                ))
            }
        },
        None => None,
    };
    let slug_of_path = match worktree {
        Some(path) => match lane_slug_under_root(canonical, path) {
            Some(slug) => Some(slug),
            None => {
                return Some(format!(
                    "'{path}' is outside the Domios worktree root, not managed"
                ))
            }
        },
        None => None,
    };
    if ownership == Some(LaneOwnership::DomiosLane) {
        return None;
    }
    let unrecorded = "lane ownership is not recorded and the lane does not match the Domios-provisioned fingerprint, not managed";
    let (Some(path), Some(path_slug)) = (worktree, slug_of_path) else {
        return Some(unrecorded.into());
    };
    let registered = match registration(path) {
        Ok(Some(registered)) => registered,
        Ok(None) => return Some(unrecorded.into()),
        Err(error) => {
            return Some(format!(
                "lane ownership could not be verified, not managed: {error}"
            ))
        }
    };
    // The lane's branch is the requested one when git agrees (or the worktree is
    // detached), otherwise whatever git has checked out there.
    let branch_slug = match (slug_of_branch, registered.as_deref()) {
        (Some(requested), None) => Some(requested),
        (Some(requested), Some(checked_out)) => {
            (checked_out.strip_prefix(LANE_BRANCH_PREFIX) == Some(requested)).then_some(requested)
        }
        (None, Some(checked_out)) => checked_out.strip_prefix(LANE_BRANCH_PREFIX),
        (None, None) => None,
    };
    if branch_slug == Some(path_slug.as_str()) {
        None
    } else {
        Some(unrecorded.into())
    }
}

/// [`unmanaged_lane_reason`] probing registration with git.
pub fn unmanaged_lane_reason_git(
    ownership: Option<LaneOwnership>,
    canonical: &str,
    worktree: Option<&str>,
    branch: Option<&str>,
) -> Option<String> {
    unmanaged_lane_reason_listed(ownership, canonical, worktree, branch, None)
}

/// [`unmanaged_lane_reason`] reading registration from a preloaded
/// `git worktree list --porcelain` of `canonical`, or from git without one.
pub fn unmanaged_lane_reason_listed(
    ownership: Option<LaneOwnership>,
    canonical: &str,
    worktree: Option<&str>,
    branch: Option<&str>,
    listing: Option<&str>,
) -> Option<String> {
    use codeterm_util::worktree_cleanup::{linked_worktree_branch, registered_worktree_branch};
    unmanaged_lane_reason(
        ownership,
        canonical,
        worktree,
        branch,
        |path| match listing {
            Some(listing) => Ok(linked_worktree_branch(listing, path)),
            None => registered_worktree_branch(canonical, path),
        },
    )
}

/// The ownership several records claim for one lane: any foreign claim wins,
/// then any unrecorded one, so merging records never widens what may be deleted.
pub fn strictest_ownership(
    left: Option<LaneOwnership>,
    right: Option<LaneOwnership>,
) -> Option<LaneOwnership> {
    match (left, right) {
        (Some(LaneOwnership::ForeignCheckout), _) | (_, Some(LaneOwnership::ForeignCheckout)) => {
            Some(LaneOwnership::ForeignCheckout)
        }
        (None, _) | (_, None) => None,
        _ => Some(LaneOwnership::DomiosLane),
    }
}

#[cfg(test)]
mod tests;
