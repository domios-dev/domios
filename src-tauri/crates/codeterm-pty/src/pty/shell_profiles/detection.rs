use super::ShellProfile;

fn native(id: &str, name: &str, executable: String) -> ShellProfile {
    ShellProfile {
        id: id.into(),
        name: name.into(),
        executable,
        args: Vec::new(),
        wsl_distro: None,
    }
}

#[cfg(any(test, target_os = "windows"))]
pub(super) fn parse_wsl_distros(bytes: &[u8]) -> Vec<String> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.contains(&0) {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        let words: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let mut distros = Vec::new();
    for name in text
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}'))
    {
        if super::paths::valid_distro(name)
            && !distros
                .iter()
                .any(|d: &String| d.eq_ignore_ascii_case(name))
        {
            distros.push(name.to_owned());
        }
    }
    distros
}

#[cfg(any(test, not(target_os = "windows")))]
pub(super) fn parse_unix_shells(
    text: &str,
    executable: impl Fn(&str) -> bool,
) -> Vec<ShellProfile> {
    let mut profiles = Vec::new();
    for path in text.lines().map(str::trim).filter(|s| s.starts_with('/')) {
        let id = path.rsplit('/').next().unwrap_or_default();
        if matches!(id, "zsh" | "bash" | "fish")
            && executable(path)
            && !profiles.iter().any(|p: &ShellProfile| p.id == id)
        {
            profiles.push(native(id, id, path.into()));
        }
    }
    profiles
}

#[cfg(not(target_os = "windows"))]
pub(super) fn detect() -> Vec<ShellProfile> {
    use std::os::unix::fs::PermissionsExt;
    let text = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    parse_unix_shells(&text, |path| {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

#[cfg(target_os = "windows")]
pub(super) fn detect() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();
    let mut add = |id, name, paths: Vec<String>, exe| {
        if let Some(path) = paths
            .into_iter()
            .find(|p| std::path::Path::new(p).is_file())
            .or_else(|| super::super::login_shell::which_exe(exe))
        {
            profiles.push(native(id, name, path));
        }
    };
    add(
        "pwsh",
        "PowerShell 7",
        vec![
            r"C:\Program Files\PowerShell\7\pwsh.exe".into(),
            r"C:\Program Files (x86)\PowerShell\7\pwsh.exe".into(),
        ],
        "pwsh.exe",
    );
    add(
        "powershell",
        "Windows PowerShell",
        Vec::new(),
        "powershell.exe",
    );
    add(
        "cmd",
        "Command Prompt",
        std::env::var("COMSPEC").into_iter().collect(),
        "cmd.exe",
    );
    let mut git_paths: Vec<String> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .flat_map(|root| {
            [
                format!(r"{root}\Git\bin\bash.exe"),
                format!(r"{root}\Programs\Git\bin\bash.exe"),
            ]
        })
        .collect();
    if let Some(git) = super::super::login_shell::which_exe("git.exe") {
        if let Some(root) = std::path::Path::new(&git)
            .parent()
            .and_then(std::path::Path::parent)
        {
            git_paths.push(
                root.join("bin")
                    .join("bash.exe")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    if let Some(path) = git_paths
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
    {
        profiles.push(native("git-bash", "Git Bash", path));
    }
    if let Some(wsl) = super::super::login_shell::which_exe("wsl.exe") {
        let mut command = std::process::Command::new(&wsl);
        command.args(["--list", "--quiet"]);
        codeterm_util::process_util::no_window_std(&mut command);
        if let Ok(output) = command.output() {
            if output.status.success() {
                profiles.extend(parse_wsl_distros(&output.stdout).into_iter().map(|distro| {
                    ShellProfile {
                        id: format!("wsl:{distro}"),
                        name: format!("WSL: {distro}"),
                        executable: wsl.clone(),
                        args: vec!["--distribution".into(), distro.clone()],
                        wsl_distro: Some(distro),
                    }
                }));
            }
        }
    }
    profiles
}
