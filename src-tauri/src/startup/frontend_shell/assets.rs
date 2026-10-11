use std::{borrow::Cow, collections::BTreeMap, path::Path};
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};

pub(super) struct DevelopmentAssets {
    files: BTreeMap<AssetKey, Vec<u8>>,
}

impl DevelopmentAssets {
    pub(super) fn load(root: &Path) -> std::io::Result<Self> {
        let mut assets = Self {
            files: BTreeMap::new(),
        };
        assets.collect(root, root)?;
        if !assets.files.contains_key(&AssetKey::from("index.html")) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Prepared development fallback index is missing",
            ));
        }
        Ok(assets)
    }

    fn collect(&mut self, root: &Path, directory: &Path) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                self.collect(root, &path)?;
            } else if kind.is_file() {
                let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
                self.files
                    .insert(AssetKey::from(relative), std::fs::read(path)?);
            }
        }
        Ok(())
    }
}

impl tauri::Assets<tauri::Wry> for DevelopmentAssets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        self.files
            .get(key)
            .map(|bytes| Cow::Borrowed(bytes.as_slice()))
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(
            self.files
                .iter()
                .map(|(key, bytes)| (Cow::Borrowed(key.as_ref()), Cow::Borrowed(bytes.as_slice()))),
        )
    }

    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

#[cfg(test)]
mod tests;
