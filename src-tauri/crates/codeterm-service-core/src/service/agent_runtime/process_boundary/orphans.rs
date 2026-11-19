use std::collections::HashMap;

/// One runtime descendant captured before a Stop, observed again after the provider settled it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub pid: u32,
    pub parent: u32,
    pub infrastructure: bool,
    pub exited: bool,
}

/// Live processes whose captured non-infrastructure ancestor ended during the Stop: they escaped the
/// provider's teardown of the interrupted tool, while chains with surviving ancestors stay retained.
pub(super) fn orphaned(nodes: &[Node]) -> Vec<usize> {
    let index: HashMap<u32, usize> = nodes
        .iter()
        .enumerate()
        .map(|(position, node)| (node.pid, position))
        .collect();
    (0..nodes.len())
        .filter(|&position| {
            let node = nodes[position];
            if node.exited || node.infrastructure {
                return false;
            }
            let mut parent = node.parent;
            for _ in 0..nodes.len() {
                let Some(&ancestor) = index.get(&parent) else {
                    return false;
                };
                let ancestor = nodes[ancestor];
                if ancestor.exited && !ancestor.infrastructure {
                    return true;
                }
                parent = ancestor.parent;
            }
            false
        })
        .collect()
}

/// A Stop completes only once every reaped tool process is confirmed gone.
pub(super) fn confirm_reaped(survivors: &[u32]) -> Result<(), String> {
    if survivors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "interrupted turn tool processes {survivors:?} survived Stop"
        ))
    }
}

#[cfg(test)]
mod tests;
