pub(crate) fn render_model_catalogue_error(error: &str) -> Option<String> {
    if !error.contains("authoritative catalogue") {
        return None;
    }
    let provider = error
        .split_once("provider '")?
        .1
        .split_once('\'')?
        .0;
    Some(format!(
        "✗ {error}\nfix: codeterm agent models {provider}"
    ))
}

#[cfg(test)]
mod tests {
    use super::render_model_catalogue_error;

    #[test]
    fn model_catalogue_refusal_prints_close_ids_and_a_recovery_command() {
        let rendered = render_model_catalogue_error(
            "model id 'gpt-9-bogus' is not in provider 'codex' authoritative catalogue; closest catalogue ids: gpt-6-sol, gpt-6-astra",
        )
        .unwrap();
        assert!(rendered.starts_with("✗ model id 'gpt-9-bogus'"));
        assert!(rendered.contains("gpt-6-sol"));
        assert!(rendered.contains("fix: codeterm agent models codex"));
    }
}
