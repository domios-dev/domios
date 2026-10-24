use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::{close, git, TeamCloseLane};

#[derive(Default)]
pub struct Inventory {
    pub worktrees: String,
    pub refs: HashMap<String, String>,
    remotes: HashMap<String, HashMap<String, String>>,
    online: bool,
    parents: HashMap<String, Vec<String>>,
    ancestors: RefCell<HashMap<String, HashSet<String>>>,
}

impl Inventory {
    pub fn load(project: &str, online: bool) -> Result<Self, String> {
        Self::load_with(project, online, git)
    }

    pub(super) fn load_with(
        project: &str,
        online: bool,
        mut query: impl FnMut(&str, &[&str]) -> Result<String, String>,
    ) -> Result<Self, String> {
        let worktrees = query(project, &["worktree", "list", "--porcelain"])?;
        let refs = parse_refs(&query(
            project,
            &["for-each-ref", "--format=%(objectname) %(refname)"],
        )?);
        let graph = query(project, &["rev-list", "--all", "--parents", "HEAD"])?;
        let parents = graph
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                let tip = fields.next().unwrap_or_default().to_string();
                (tip, fields.map(str::to_string).collect())
            })
            .collect();
        let mut remotes = HashMap::new();
        for remote in query(project, &["remote"])?.lines() {
            let tips = if online {
                parse_refs(&query(
                    project,
                    &["ls-remote", "--refs", remote, "refs/heads/*"],
                )?)
            } else {
                let prefix = format!("refs/remotes/{remote}/");
                refs.iter()
                    .filter_map(|(name, tip)| {
                        name.strip_prefix(&prefix)
                            .map(|branch| (format!("refs/heads/{branch}"), tip.clone()))
                    })
                    .collect()
            };
            remotes.insert(remote.to_string(), tips);
        }
        Ok(Self {
            worktrees,
            refs,
            remotes,
            online,
            parents,
            ancestors: RefCell::default(),
        })
    }

    pub fn worktree_paths(&self) -> Vec<String> {
        self.worktrees
            .lines()
            .filter_map(|line| line.strip_prefix("worktree ").map(str::to_string))
            .collect()
    }

    pub fn absent(&self, lane: &TeamCloseLane) -> bool {
        lane.worktree_path.as_deref().is_none_or(|path| {
            std::fs::symlink_metadata(path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                && !self
                    .worktrees
                    .lines()
                    .filter_map(|line| line.strip_prefix("worktree "))
                    .any(|registered| {
                        codeterm_service_core::service::worker_lane::same_path(registered, path)
                    })
        }) && lane.branch.as_deref().is_none_or(|branch| {
            let reference = format!("refs/heads/{branch}");
            !self.refs.contains_key(&reference)
                && self
                    .remotes
                    .values()
                    .all(|tips| !tips.contains_key(&reference))
        })
    }

    fn resolve(&self, cwd: &str, reference: &str) -> Option<String> {
        if reference == "HEAD" {
            let mut current = None;
            for line in self.worktrees.lines() {
                if let Some(path) = line.strip_prefix("worktree ") {
                    current = Some(path);
                }
                if let Some(tip) = line.strip_prefix("HEAD ") {
                    if current.is_some_and(|path| {
                        codeterm_service_core::service::worker_lane::same_path(path, cwd)
                    }) {
                        return Some(tip.into());
                    }
                }
            }
            return None;
        }
        if let Some(tip) = self.refs.get(reference) {
            return Some(tip.clone());
        }
        if self.parents.contains_key(reference) {
            return Some(reference.into());
        }
        if self.refs.contains_key(&format!("refs/tags/{reference}"))
            || self.refs.contains_key(&format!("refs/{reference}"))
        {
            return None;
        }
        self.refs.get(&format!("refs/heads/{reference}")).cloned()
    }

    pub fn merged_refusal(&self, cwd: &str, reference: &str, destination: &str) -> Option<String> {
        if reference.starts_with('-') || destination.starts_with('-') {
            return Some("invalid lane ancestry reference".into());
        }
        let resolved = self
            .resolve(cwd, reference)
            .zip(self.resolve(cwd, destination));
        let Some((tip, target)) = resolved.filter(|(tip, target)| {
            self.parents.contains_key(tip) && self.parents.contains_key(target)
        }) else {
            return close::merged_refusal(cwd, reference, destination);
        };
        let mut cache = self.ancestors.borrow_mut();
        let reachable = cache.entry(target.clone()).or_insert_with(|| {
            let mut reachable = HashSet::new();
            let mut pending = vec![target];
            while let Some(commit) = pending.pop() {
                if reachable.insert(commit.clone()) {
                    if let Some(parents) = self.parents.get(&commit) {
                        pending.extend(parents.iter().cloned());
                    }
                }
            }
            reachable
        });
        (!reachable.contains(&tip))
            .then(|| format!("branch has unmerged commits ahead of destination {destination}"))
    }

    pub fn remote_tips(&self, lane: &TeamCloseLane) -> Result<Vec<(String, String)>, String> {
        let Some(branch) = lane.branch.as_deref() else {
            return Ok(Vec::new());
        };
        let reference = format!("refs/heads/{branch}");
        let mut tips = Vec::new();
        for (remote, branches) in &self.remotes {
            if let Some(tip) = branches.get(&reference) {
                let refusal = if self.online {
                    close::merged_refusal(
                        &lane.project_path,
                        tip,
                        lane.destination.as_deref().unwrap_or("HEAD"),
                    )
                } else {
                    self.merged_refusal(
                        &lane.project_path,
                        tip,
                        lane.destination.as_deref().unwrap_or("HEAD"),
                    )
                };
                if let Some(reason) = refusal {
                    return Err(format!("remote {remote}: {reason}"));
                }
                tips.push((remote.clone(), tip.clone()));
            }
        }
        tips.sort();
        Ok(tips)
    }
}

fn parse_refs(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let tip = fields.next()?;
            Some((fields.next()?.to_string(), tip.to_string()))
        })
        .collect()
}
