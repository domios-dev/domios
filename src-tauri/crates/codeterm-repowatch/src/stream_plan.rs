use codeterm_util::watch_plan::{DirectoryPlan, ExcludedDirectory};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) const MAX_EXCLUSIONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StreamSpec {
    pub root: PathBuf,
    pub exclusions: Vec<PathBuf>,
}

fn stream(root: &Path, candidates: &[ExcludedDirectory]) -> Result<StreamSpec, String> {
    let heavy = candidates
        .iter()
        .filter(|candidate| candidate.heavy)
        .count();
    if heavy > MAX_EXCLUSIONS {
        let error = format!(
            "FSEvents subtree {} needs {heavy} heavy exclusions; maximum {MAX_EXCLUSIONS}",
            root.display()
        );
        log::warn!("{error}");
        return Err(error);
    }
    let mut candidates = candidates.to_vec();
    candidates.sort_by_key(|candidate| (!candidate.heavy, candidate.path.clone()));
    Ok(StreamSpec {
        root: root.to_path_buf(),
        exclusions: candidates
            .into_iter()
            .take(MAX_EXCLUSIONS)
            .map(|candidate| candidate.path)
            .collect(),
    })
}

pub(crate) fn plan(root: &Path, directory: &DirectoryPlan) -> Result<Vec<StreamSpec>, String> {
    if directory
        .excluded
        .iter()
        .filter(|candidate| candidate.heavy)
        .count()
        <= MAX_EXCLUSIONS
    {
        return Ok(vec![stream(root, &directory.excluded)?]);
    }
    let mut delegated = BTreeSet::new();
    for candidate in directory
        .excluded
        .iter()
        .filter(|candidate| candidate.heavy)
    {
        let relative = candidate
            .path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?;
        let child = relative
            .components()
            .next()
            .ok_or("empty FSEvents exclusion")?;
        delegated.insert(root.join(child.as_os_str()));
    }
    let mut root_candidates: Vec<_> = directory
        .excluded
        .iter()
        .filter(|candidate| {
            !delegated
                .iter()
                .any(|bucket| candidate.path.starts_with(bucket))
        })
        .cloned()
        .collect();
    root_candidates.extend(delegated.iter().map(|path| ExcludedDirectory {
        path: path.clone(),
        heavy: true,
    }));
    let mut result = vec![stream(root, &root_candidates)?];
    for child in delegated {
        if directory
            .excluded
            .iter()
            .any(|candidate| candidate.path == child)
        {
            continue;
        }
        let candidates: Vec<_> = directory
            .excluded
            .iter()
            .filter(|candidate| candidate.path.starts_with(&child))
            .cloned()
            .collect();
        result.push(stream(&child, &candidates)?);
    }
    result.sort();
    Ok(result)
}

pub(crate) trait StreamPort {
    type Stream;
    fn create(&mut self, root: &Path) -> Result<Self::Stream, String>;
    fn exclude(&mut self, stream: &mut Self::Stream, paths: &[PathBuf]) -> Result<(), String>;
    fn start(&mut self, stream: &mut Self::Stream) -> Result<(), String>;
}

pub(crate) fn start<P: StreamPort>(port: &mut P, spec: &StreamSpec) -> Result<P::Stream, String> {
    if spec.exclusions.len() > MAX_EXCLUSIONS {
        return Err("FSEvents exclusion limit exceeded before stream creation".into());
    }
    let mut stream = port.create(&spec.root)?;
    port.exclude(&mut stream, &spec.exclusions)?;
    port.start(&mut stream)?;
    Ok(stream)
}

#[cfg(test)]
mod tests;
