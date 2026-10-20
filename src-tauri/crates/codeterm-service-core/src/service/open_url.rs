pub fn open_url(url: String) -> Result<(), String> {
    let (program, args) = opener_command(&url);
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    #[cfg(target_os = "windows")]
    crate::process_util::no_window_std(&mut cmd);
    let _ = cmd.spawn();
    Ok(())
}

/// `cmd /C start` splits a URL at `&`, dropping every query parameter after the first.
fn opener_command(url: &str) -> (&'static str, Vec<String>) {
    if cfg!(target_os = "windows") {
        (
            "rundll32",
            vec!["url.dll,FileProtocolHandler".into(), url.into()],
        )
    } else if cfg!(target_os = "macos") {
        ("open", vec![url.into()])
    } else {
        ("xdg-open", vec![url.into()])
    }
}

#[cfg(test)]
mod tests {
    use super::opener_command;

    #[test]
    fn opener_passes_the_whole_query_string_as_one_argument() {
        let url = "https://example.test/signin/?state=a&code_challenge=b&redirect_uri=c";
        let (program, args) = opener_command(url);
        assert_ne!(program, "cmd");
        assert_eq!(args.last().map(String::as_str), Some(url));
    }
}
