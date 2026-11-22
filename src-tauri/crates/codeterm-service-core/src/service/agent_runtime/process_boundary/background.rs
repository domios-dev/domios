use super::*;
use std::collections::{BTreeMap, HashSet};

impl Boundary {
    pub(in crate::service::agent_runtime) fn capture_background(
        &self,
        target: &RuntimeBackgroundProcess,
        others: &[RuntimeBackgroundProcess],
    ) -> Result<Option<BackgroundTree>, RuntimeFailure> {
        let tree = self.capture_tools()?;
        let system = process_commands(tree.nodes.iter().map(|node| node.pid));
        let mut nodes = Vec::with_capacity(tree.nodes.len());
        let mut candidates = Vec::new();
        let mut protected = HashSet::new();
        for (position, node) in tree.nodes.iter().enumerate() {
            let exited = node.member.exited().map_err(failure)?;
            let observed = process_command(&system, node.pid);
            let matches = |process: &RuntimeBackgroundProcess| {
                if let Some(pid) = process.os_pid {
                    return pid == node.pid;
                }
                observed.as_ref().is_some_and(|(argv, cwd)| {
                    cwd.as_deref().is_some_and(|cwd| {
                        selective::shell_matches(
                            argv,
                            cwd,
                            &process.command,
                            std::path::Path::new(&process.cwd),
                        )
                    })
                })
            };
            if !exited && matches(target) {
                candidates.push(position);
            }
            if !exited && others.iter().any(matches) {
                protected.insert(position);
            }
            nodes.push(orphans::Node {
                pid: node.pid,
                parent: node.parent,
                infrastructure: self.infrastructure_process(node.pid, &node.member),
                exited,
            });
        }
        let Some(branch) = selective::subtree(&nodes, &candidates, &protected) else {
            return Ok(None);
        };
        let branch: HashSet<_> = branch.into_iter().collect();
        let nodes = tree
            .nodes
            .into_iter()
            .enumerate()
            .filter_map(|(position, node)| branch.contains(&position).then_some(node))
            .collect();
        Ok(Some(BackgroundTree {
            tree: ToolTree { nodes },
        }))
    }

    pub(in crate::service::agent_runtime) fn reap_background(
        &self,
        capture: BackgroundTree,
    ) -> Result<(), RuntimeFailure> {
        let mut nodes = capture.tree.nodes;
        let deadline = Instant::now() + Duration::from_secs(5);
        for _ in 0..4 {
            let retained: BTreeMap<_, _> =
                nodes.iter().map(|node| (node.pid, &node.member)).collect();
            let survivors = terminate_retained(&retained, deadline)?;
            if !survivors.is_empty() {
                return Err(failure(format!(
                    "background command processes {survivors:?} survived Stop"
                )));
            }
            let late: Vec<_> = native::descendants_of(nodes.iter())
                .map_err(failure)?
                .into_iter()
                .filter(|node| !retained.contains_key(&node.pid))
                .collect();
            if late.is_empty() {
                return Ok(());
            }
            if nodes.len() + late.len() > 256
                || Instant::now() >= deadline
                || late
                    .iter()
                    .any(|node| self.infrastructure_process(node.pid, &node.member))
            {
                break;
            }
            nodes.extend(late);
        }
        Err(failure(
            "background command subtree exit could not be confirmed",
        ))
    }
}
