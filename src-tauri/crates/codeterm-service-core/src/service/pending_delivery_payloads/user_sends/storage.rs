use super::*;

impl UserSendStore {
    pub(super) fn load<'a>(
        &self,
        tab: &UserTab,
        data: &'a mut Option<LoadedIndex>,
    ) -> Result<&'a mut LoadedIndex, String> {
        if tab.purged.load(Ordering::Acquire) {
            return Err("user send owner is revoked: closed tab payloads were purged".into());
        }
        if tab.failed.load(Ordering::Acquire) {
            self.recover(tab, data)
                .map_err(|error| tab.fail_storage(error))?;
        }
        if data.is_none() {
            {
                let mut initialized = self.root_initialized.lock();
                if !*initialized {
                    owner_directory(&self.root)?;
                    *initialized = true;
                }
            }
            owner_directory(&tab.dir)?;
            let path = tab.dir.join("index.json");
            let mut index: UserIndex = match fs::symlink_metadata(&path) {
                Ok(_) => serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                    .map_err(|_| "user index is invalid")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut entries = fs::read_dir(&tab.dir).map_err(io_error)?;
                    if entries.next().transpose().map_err(io_error)?.is_some() {
                        return Err("user index absent with retained files; owner held".into());
                    }
                    UserIndex::default()
                }
                Err(error) => return Err(io_error(error)),
            };
            if index.version != 1 {
                return Err("user index version is unknown".into());
            }
            index
                .source_claims
                .retain(|claim| claim.matched_row.source.valid());
            validate_index(&index, &tab.tab_id)?;
            let sequence_base = index.next_sequence;
            tab.unsettled
                .store(unsettled_count(&index), Ordering::Release);
            *data = Some(LoadedIndex {
                index,
                sequence_base,
            });
            if let Err(error) =
                self.collect_orphans(tab, &data.as_ref().ok_or("user index not loaded")?.index)
            {
                return Err(tab.fail_storage(error));
            }
        }
        let loaded = data.as_mut().ok_or("user index not loaded")?;
        tab.storage_qualified.store(true, Ordering::Release);
        Ok(loaded)
    }

    fn collect_orphans(&self, tab: &UserTab, index: &UserIndex) -> Result<(), String> {
        let entries = fs::read_dir(&tab.dir)
            .map_err(io_error)?
            .take(MAX_DIRECTORY_ENTRIES + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        if entries.len() > MAX_DIRECTORY_ENTRIES {
            return Err("user directory work bound exceeded".into());
        }
        let live: HashSet<String> = index
            .records
            .iter()
            .flat_map(|record| {
                ["original", "prepared", "body"]
                    .map(|suffix| format!("{}.{suffix}", record.send_id))
            })
            .collect();
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == "index.json" || live.contains(&name) {
                continue;
            }
            let owned = name == "index.next.json"
                || name.rsplit_once('.').is_some_and(|(id, suffix)| {
                    uuid::Uuid::parse_str(id).is_ok()
                        && matches!(suffix, "original" | "prepared" | "body")
                });
            if owned && entry.file_type().map_err(io_error)?.is_file() {
                fs::remove_file(entry.path()).map_err(io_error)?;
            }
        }
        Ok(())
    }

    pub(super) fn persist(&self, tab: &UserTab, index: &UserIndex) -> Result<(), String> {
        let result = (|| {
            self.boundary("index")?;
            let bytes = serde_json::to_vec(index).map_err(|_| "user index serialization failed")?;
            if bytes.len() as u64 > MAX_INDEX_BYTES {
                return Err("user index size bound exceeded".into());
            }
            let temporary = tab.dir.join("index.next.json");
            write_replace(&temporary, &bytes)?;
            atomic_replace(&temporary, &tab.dir.join("index.json"))?;
            self.boundary("index_replaced")?;
            sync_directory(&tab.dir)
        })();
        result.map_err(|error| tab.fail_storage(error))
    }

    fn recover(&self, tab: &UserTab, data: &mut Option<LoadedIndex>) -> Result<(), String> {
        if tab.revoked.load(Ordering::Acquire) {
            return Err("user send owner is revoked".into());
        }
        owner_directory(&tab.dir)?;
        let path = tab.dir.join("index.json");
        let mut index: UserIndex = match fs::symlink_metadata(&path) {
            Ok(_) => serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                .map_err(|_| "user index is invalid")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let previous = data.as_ref().ok_or("user index absent; owner held")?;
                if previous.index.next_sequence != 1 {
                    return Err("user index absent with accepted sends; owner held".into());
                }
                previous.index.clone()
            }
            Err(error) => return Err(io_error(error)),
        };
        if index.version != 1 {
            return Err("user index version is unknown".into());
        }
        index
            .source_claims
            .retain(|claim| claim.matched_row.source.valid());
        validate_index(&index, &tab.tab_id)?;
        let previous = data
            .as_ref()
            .ok_or("user index recovery has no prior qualification")?;
        for record in &mut index.records {
            if record.state == UserSendState::Queued
                && !previous.index.records.iter().any(|prior| {
                    prior.send_id == record.send_id && prior.state == UserSendState::Queued
                })
            {
                record.state = UserSendState::Uncertain;
            }
        }
        self.collect_orphans(tab, &index)?;
        self.persist(tab, &index)?;
        tab.unsettled
            .store(unsettled_count(&index), Ordering::Release);
        *data = Some(LoadedIndex {
            index,
            sequence_base: previous.sequence_base,
        });
        tab.failed.store(false, Ordering::Release);
        *tab.storage_error.lock() = None;
        Ok(())
    }
}

impl UserTab {
    pub(super) fn fail_storage(&self, error: String) -> String {
        let error = if error.contains("send storage write failed:") {
            error
        } else {
            format!("send storage write failed: {error}; retry when storage is writable")
        };
        *self.storage_error.lock() = Some(error.clone());
        self.failed.store(true, Ordering::Release);
        error
    }

    pub(super) fn check_available(&self) -> Result<(), String> {
        if self.revoked.load(Ordering::Acquire) {
            return Err("user send owner is revoked".into());
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(self.storage_error.lock().clone().unwrap_or_else(|| {
                "send storage write failed: unqualified storage; retry when storage is writable"
                    .into()
            }));
        }
        Ok(())
    }
}
