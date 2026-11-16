use codeterm_util::watch_plan::PathCase;
use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub path: PathBuf,
    pub directory: Option<bool>,
    pub replaced: bool,
}

#[derive(Default)]
pub(crate) struct PendingChanges(BTreeMap<String, Change>);

impl PendingChanges {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn record(&mut self, event: &Event) {
        let (directory, replaced) = match event.kind {
            EventKind::Access(_) => return,
            EventKind::Create(CreateKind::File) => (Some(false), false),
            EventKind::Create(CreateKind::Folder) => (Some(true), false),
            EventKind::Remove(RemoveKind::File) => (Some(false), true),
            EventKind::Remove(RemoveKind::Folder) => (Some(true), true),
            EventKind::Modify(ModifyKind::Name(_)) => {
                let file_destination = event.paths.iter().any(|path| path.is_file());
                let directory_destination = event.paths.iter().any(|path| path.is_dir());
                (
                    (file_destination && !directory_destination).then_some(false),
                    true,
                )
            }
            EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
                (Some(false), false)
            }
            EventKind::Remove(_) => (None, true),
            _ => (None, false),
        };
        for path in &event.paths {
            let key = PathCase::native().key(path);
            self.0
                .entry(key)
                .and_modify(|change| {
                    change.directory = match (change.directory, directory) {
                        (Some(true), _) | (_, Some(true)) => Some(true),
                        (Some(false), Some(false)) => Some(false),
                        _ => None,
                    };
                    change.replaced |= replaced;
                })
                .or_insert_with(|| Change {
                    path: path.clone(),
                    directory,
                    replaced,
                });
        }
    }

    pub fn take(&mut self, paths: &[PathBuf]) -> Vec<Change> {
        paths
            .iter()
            .filter_map(|path| self.0.remove(&PathCase::native().key(path)))
            .collect()
    }
}
