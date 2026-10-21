#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CuratorNote {
    pub next_secs: u64,
    pub reason: String,
    pub next_move: String,
    pub ready: Option<String>,
    pub retro: Vec<String>,
    pub learnings: Vec<String>,
    pub next_checks: Vec<String>,
    pub tidy: Vec<String>,
    pub candidates: Vec<String>,
    pub asked: String,
}

pub fn duration_secs(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut number = String::new();
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().ok()?;
        number.clear();
        total = total.checked_add(n.checked_mul(match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => return None,
        })?)?;
    }
    if !number.is_empty() {
        return None;
    }
    (total > 0).then_some(total)
}

fn has_ids(text: &str) -> bool {
    text.rsplit_once('[').is_some_and(|(_, ids)| {
        ids.split_once(']')
            .is_some_and(|(ids, _)| !ids.trim().is_empty())
    })
}

pub fn parse(text: &str, requested_secs: u64) -> Result<CuratorNote, String> {
    parse_for_round(text, requested_secs, false)
}

pub fn parse_for_round(
    text: &str,
    requested_secs: u64,
    synthesis: bool,
) -> Result<CuratorNote, String> {
    if text.len() > 8192 {
        return Err("Curator note exceeds 8 KiB".into());
    }
    let mut note = CuratorNote::default();
    let mut section = "";
    let mut next_seen = false;
    let mut asked_seen = false;
    for raw in text.lines().filter(|s| !s.trim().is_empty()) {
        let raw = raw.trim();
        if raw.len() > 512 {
            return Err("Curator note line exceeds 512 bytes".into());
        }
        let mut line = raw;
        if let Some((head, value)) = raw.split_once(':') {
            if [
                "NEXT",
                "MOVE",
                "READY",
                "RETRO",
                "LEARN",
                "NEXT-CHECK",
                "TIDY",
                "CANDIDATES",
                "ASKED",
            ]
            .contains(&head)
            {
                section = head;
                line = value.trim();
                if head == "NEXT" {
                    if next_seen {
                        return Err("duplicate NEXT".into());
                    }
                    next_seen = true;
                    let (duration, reason) =
                        line.split_once('|').ok_or("NEXT needs duration | reason")?;
                    note.next_secs = duration_secs(duration).ok_or("invalid NEXT duration")?;
                    if note.next_secs != requested_secs || reason.trim().is_empty() {
                        return Err("NEXT must match --next and include a reason".into());
                    }
                    note.reason = reason.trim().into();
                    continue;
                }
                if head == "ASKED" {
                    if asked_seen || !["none", "answered", "skip", "timeout"].contains(&line) {
                        return Err("invalid ASKED".into());
                    }
                    asked_seen = true;
                    note.asked = line.into();
                    continue;
                }
                if head == "MOVE" {
                    if !note.next_move.is_empty() || !has_ids(line) {
                        return Err("MOVE requires one next step with cited ids".into());
                    }
                    note.next_move = line.into();
                    continue;
                }
                if head == "READY" {
                    let verdict = line.split_whitespace().next();
                    if !synthesis
                        || note.ready.is_some()
                        || !matches!(verdict, Some("yes" | "no"))
                        || !has_ids(line)
                    {
                        return Err(
                            "READY requires a synthesis round, yes or no, and cited ids".into()
                        );
                    }
                    note.ready = Some(line.into());
                    continue;
                }
            }
        }
        if line.is_empty() {
            continue;
        }
        match section {
            "RETRO" => {
                if !line.starts_with(['+', '-']) {
                    return Err("RETRO lines need + or -".into());
                }
                if has_ids(line) {
                    note.retro.push(line.into());
                }
            }
            "LEARN" => {
                if !line.starts_with("when ")
                    || !line.contains(" -> ")
                    || !(line.ends_with("(confidence low)")
                        || line.ends_with("(confidence medium)"))
                {
                    return Err("invalid LEARN form".into());
                }
                if has_ids(line) {
                    note.learnings.push(line.into());
                }
            }
            "NEXT-CHECK" => {
                if has_ids(line) {
                    note.next_checks.push(line.into());
                }
            }
            "TIDY" => note.tidy.push(line.into()),
            "CANDIDATES" => note.candidates.push(line.into()),
            _ => return Err("unknown Curator note line".into()),
        }
    }
    if !next_seen || !asked_seen || note.next_move.is_empty() {
        return Err("NEXT, MOVE and ASKED are required".into());
    }
    if note.retro.len() > 5
        || note.learnings.len() > 3
        || note.next_checks.len() > 5
        || note.tidy.len() > 10
        || note.candidates.len() > 3
    {
        return Err("Curator section cap exceeded".into());
    }
    Ok(note)
}
