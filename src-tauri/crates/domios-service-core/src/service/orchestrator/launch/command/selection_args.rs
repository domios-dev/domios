pub(super) fn without_managed_arguments(
    arguments: Option<&[String]>,
    managed: &[String],
) -> Option<Vec<String>> {
    let mut retained = Vec::new();
    let mut remaining = arguments.unwrap_or_default();
    while !remaining.is_empty() {
        if !managed.is_empty() && remaining.starts_with(managed) {
            remaining = &remaining[managed.len()..];
        } else {
            retained.push(remaining[0].clone());
            remaining = &remaining[1..];
        }
    }
    (!retained.is_empty()).then_some(retained)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_selection_is_typed_once_and_real_extra_arguments_survive() {
        let managed = vec!["--model".into(), "selected-model".into()];
        assert_eq!(without_managed_arguments(Some(&managed), &managed), None);
        let mixed = vec![
            "--opaque".into(),
            "selected-model".into(),
            "--model".into(),
            "selected-model".into(),
            "--other".into(),
            "value".into(),
        ];
        assert_eq!(
            without_managed_arguments(Some(&mixed), &managed),
            Some(vec![
                "--opaque".into(),
                "selected-model".into(),
                "--other".into(),
                "value".into()
            ])
        );
        let foreign = vec!["--model".into(), "other-model".into()];
        assert_eq!(
            without_managed_arguments(Some(&foreign), &managed),
            Some(foreign)
        );
        assert_eq!(without_managed_arguments(Some(&mixed), &[]), Some(mixed));
    }
}
