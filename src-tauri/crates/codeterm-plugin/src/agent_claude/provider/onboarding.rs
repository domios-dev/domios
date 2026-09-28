pub(super) fn not_now_step(screen: &str) -> Option<Vec<u8>> {
    let question = screen.lines().rev().find(|line| line.trim_end().ends_with('?'))?;
    let question = question.to_ascii_lowercase();
    if !question.split(|c: char| !c.is_ascii_alphanumeric()).any(|word| word == "auto")
        || !question
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word == "environment")
    {
        return None;
    }

    let options = screen
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let focused = trimmed.starts_with('❯') || trimmed.starts_with('>');
            let row = trimmed
                .trim_start_matches(|character| character == '❯' || character == '>')
                .trim_start();
            let (number, label) = row.split_once(". ")?;
            (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
                .then_some((focused, label.trim()))
        })
        .collect::<Vec<_>>();
    if options.len() != 3 {
        return None;
    }
    let labels = options
        .iter()
        .map(|(_, label)| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let target = labels
        .iter()
        .position(|label| label.starts_with("not now"))?;
    if labels.iter().filter(|label| label.starts_with("yes")).count() != 1
        || labels
            .iter()
            .filter(|label| label.starts_with("don't show again"))
            .count()
            != 1
    {
        return None;
    }
    let mut focused = options.iter().enumerate().filter(|(_, row)| row.0);
    let current = focused.next()?.0;
    if focused.next().is_some() {
        return None;
    }
    Some(if current == target {
        b"\r".to_vec()
    } else if target > current {
        b"j".to_vec()
    } else {
        b"k".to_vec()
    })
}
