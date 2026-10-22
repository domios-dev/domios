use std::io::Write;
use std::path::Path;

const SHELL_PATH: &str = "# Domios CLI\ncase \"$PATH\" in\n  \"$HOME/.domios/bin\"|\"$HOME/.domios/bin:\"*) ;;\n  *) export PATH=\"$HOME/.domios/bin:$PATH\" ;;\nesac\n";
const FISH_PATH: &str = "# Domios CLI\nfish_add_path --prepend \"$HOME/.domios/bin\"\n";

pub(super) fn install(home: &Path) -> std::io::Result<()> {
    for name in [".profile", ".bashrc", ".zshenv"] {
        append_entry(&home.join(name), SHELL_PATH)?;
    }
    for name in [".bash_profile", ".bash_login"] {
        let path = home.join(name);
        if path.exists() {
            append_entry(&path, SHELL_PATH)?;
        }
    }
    let fish = home.join(".config/fish/conf.d/domios.fish");
    if let Some(parent) = fish.parent() {
        std::fs::create_dir_all(parent)?;
    }
    append_entry(&fish, FISH_PATH)
}

fn append_entry(path: &Path, entry: &str) -> std::io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if existing.contains(entry) {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.write_all(entry.as_bytes())
}

#[cfg(test)]
mod tests;
