use std::process::Command;

pub fn git_command() -> Command {
    let mut command = Command::new("git");
    super::no_window_std(&mut command);
    #[cfg(windows)]
    command.args(["-c", "core.longpaths=true"]);
    command
}

pub fn git_diagnostics(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .split(['\r', '\n'])
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !is_progress(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_progress(line: &str) -> bool {
    let Some((phase, progress)) = line.split_once(": ") else {
        return false;
    };
    if !matches!(
        phase,
        "Updating files"
            | "Checking out files"
            | "Refreshing index"
            | "Counting objects"
            | "Compressing objects"
            | "Receiving objects"
            | "Resolving deltas"
    ) {
        return false;
    }
    let Some((percentage, counts)) = progress.trim_start().split_once('%') else {
        return false;
    };
    !percentage.is_empty()
        && percentage.bytes().all(|byte| byte.is_ascii_digit())
        && counts.starts_with(" (")
        && counts.contains('/')
}

#[cfg(test)]
mod tests;
