use crate::ignore_plan::Inputs;
use crate::WatchSpec;
use codeterm_util::watch_plan::{DirectoryPlan, PathCase};
use notify::Watcher;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod events;
pub(crate) use events::{Change, PendingChanges};

struct Worktree {
    inputs: Inputs,
    plan: DirectoryPlan,
}

#[derive(Default)]
pub(crate) struct Coverage {
    worktrees: BTreeMap<PathBuf, Worktree>,
}

impl Coverage {
    pub fn reconcile(
        &mut self,
        watcher: &mut (impl Watcher + ?Sized),
        active: &mut BTreeMap<PathBuf, bool>,
        specs: &[WatchSpec],
        changes: &[Change],
    ) -> Result<bool, String> {
        let desired = match self.expand(specs, changes) {
            Ok(desired) => desired,
            Err(error) => {
                self.worktrees.clear();
                for path in active.keys() {
                    let _ = watcher.unwatch(path);
                }
                active.clear();
                return Err(error);
            }
        };
        let replaced: Vec<_> = changes
            .iter()
            .filter(|change| change.replaced)
            .map(|change| change.path.clone())
            .collect();
        let before = active.len();
        crate::subscriptions::invalidate(watcher, active, &replaced)?;
        let rearmed = active.len() != before;
        crate::subscriptions::reconcile_desired(watcher, active, desired)
            .map(|changed| changed || rearmed)
    }

    #[cfg(target_os = "macos")]
    pub fn directory_plan(&self, root: &Path) -> Option<&DirectoryPlan> {
        self.worktrees.get(root).map(|worktree| &worktree.plan)
    }

    pub(crate) fn expand(
        &mut self,
        specs: &[WatchSpec],
        changes: &[Change],
    ) -> Result<BTreeMap<PathBuf, bool>, String> {
        self.worktrees.retain(|root, _| {
            specs
                .iter()
                .any(|spec| spec.partitioned && spec.path == *root)
        });
        let mut desired = BTreeMap::new();
        for spec in specs {
            if spec.optional && !spec.path.exists() {
                continue;
            }
            if !spec.partitioned {
                desired.insert(spec.path.clone(), spec.recursive);
                continue;
            }
            self.refresh(&spec.path, changes)?;
            if let Some(worktree) = self.worktrees.get(&spec.path) {
                for watch in &worktree.plan.watches {
                    desired.insert(watch.path.clone(), watch.recursive);
                }
            }
        }
        crate::subscriptions::check_limit(&desired)?;
        Ok(desired)
    }

    fn refresh(&mut self, root: &Path, changes: &[Change]) -> Result<(), String> {
        let case = PathCase::native();
        let cached = self.worktrees.get(root);
        let mut rebuild = cached.is_none();
        let mut inputs_changed = false;
        if let Some(cached) = cached {
            for change in changes {
                inputs_changed |= case.same(&change.path, &cached.inputs.index)
                    || case.same(&change.path, &cached.inputs.config);
                rebuild |= cached
                    .inputs
                    .sources
                    .iter()
                    .any(|path| case.same(path, &change.path))
                    || (case.within(&change.path, root)
                        && change.path.file_name().is_some_and(|name| {
                            case.same(Path::new(name), Path::new(".gitignore"))
                        }))
                    || directory_changed(root, cached, change, case);
            }
        }
        if !rebuild && !inputs_changed {
            return Ok(());
        }
        let inputs = Inputs::read(root)?;
        if !rebuild && cached.is_some_and(|cached| cached.inputs == inputs) {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let plan = crate::ignore_plan::build(root, &inputs)?;
        log::debug!(
            "[repowatch] coverage rebuilt root={} tracked={} watches={} elapsed_ms={}",
            root.display(),
            inputs.tracked.len(),
            plan.watches.len(),
            started.elapsed().as_millis()
        );
        self.worktrees
            .insert(root.to_path_buf(), Worktree { inputs, plan });
        Ok(())
    }
}

fn directory_changed(root: &Path, cached: &Worktree, change: &Change, case: PathCase) -> bool {
    if case.within(&change.path, &cached.inputs.gitdir)
        || case.within(&change.path, &cached.inputs.common)
        || !(case.within(&change.path, root) || case.within(root, &change.path))
    {
        return false;
    }
    if change.path.is_symlink() {
        return true;
    }
    let covered_directory = cached
        .plan
        .watches
        .iter()
        .any(|watch| case.within(&watch.path, &change.path))
        || cached
            .plan
            .excluded
            .iter()
            .any(|excluded| case.within(&excluded.path, &change.path));
    if change.replaced && covered_directory {
        return true;
    }
    match change.directory {
        Some(directory) => directory,
        None => change.path.is_dir() || covered_directory,
    }
}

#[cfg(test)]
mod tests;
