use std::path::Path;

pub(super) fn portable_path(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut path = if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    };
    if path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic() {
        path[..1].make_ascii_lowercase();
    }
    path
}

#[cfg(test)]
mod tests;
