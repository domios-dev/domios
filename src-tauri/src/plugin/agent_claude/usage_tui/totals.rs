use regex::Regex;

#[derive(Debug, Default)]
pub struct Totals {
    pub cost_usd: Option<f32>,
    pub api_duration_s: Option<i64>,
    pub wall_duration_s: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub tokens_input: Option<i64>,
    pub tokens_output: Option<i64>,
    pub tokens_cache_read: Option<i64>,
    pub tokens_cache_write: Option<i64>,
}

pub fn parse_totals(text: &str) -> Totals {
    let mut t = Totals::default();
    t.cost_usd = match_f32(text, r"Total\s*cost\s*:?\s*\$(\d+(?:\.\d+)?)");
    t.api_duration_s = parse_duration(text, r"Total\s*durat[io]?on\s*\(\s*API\s*\)\s*:?\s*([\dhms\.\s]+)");
    t.wall_duration_s = parse_duration(text, r"Total\s*durat[io]?on\s*\(\s*wall\s*\)\s*:?\s*([\dhms\.\s]+)");

    let (added, removed) = parse_code_changes(text);
    t.lines_added = added;
    t.lines_removed = removed;

    let (i, o, cr, cw) = parse_tokens(text);
    t.tokens_input = i;
    t.tokens_output = o;
    t.tokens_cache_read = cr;
    t.tokens_cache_write = cw;
    t
}

fn match_f32(text: &str, pattern: &str) -> Option<f32> {
    let re = Regex::new(pattern).ok()?;
    re.captures(text)?
        .get(1)?
        .as_str()
        .parse::<f32>()
        .ok()
}

fn parse_duration(text: &str, pattern: &str) -> Option<i64> {
    let re = Regex::new(pattern).ok()?;
    let raw = re.captures(text)?.get(1)?.as_str().trim().to_string();
    parse_dur_str(&raw)
}

fn parse_dur_str(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    let mut num = String::new();
    let mut found_any = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
        } else if ch == 'h' || ch == 'H' {
            let n: f64 = num.trim().parse().ok()?;
            total += (n * 3600.0) as i64;
            num.clear();
            found_any = true;
        } else if ch == 'm' || ch == 'M' {
            let n: f64 = num.trim().parse().ok()?;
            total += (n * 60.0) as i64;
            num.clear();
            found_any = true;
        } else if ch == 's' || ch == 'S' {
            let n: f64 = num.trim().parse().ok()?;
            total += n as i64;
            num.clear();
            found_any = true;
        } else if ch.is_whitespace() {
            continue;
        } else {
            return None;
        }
    }
    if !num.trim().is_empty() {
        let n: f64 = num.trim().parse().ok()?;
        total += n as i64;
        found_any = true;
    }
    if found_any {
        Some(total)
    } else {
        None
    }
}

fn parse_code_changes(text: &str) -> (Option<i64>, Option<i64>) {
    let re = match Regex::new(r"Total\s*code\s*changes\s*:?\s*(\d+)\s*lines?\s*add(?:e?d|d)?\s*,?\s*(\d+)\s*lines?\s*remov(?:e?d|d|e)?")
    {
        Ok(r) => r,
        Err(_) => return (None, None),
    };
    if let Some(caps) = re.captures(text) {
        let added = caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok());
        let removed = caps.get(2).and_then(|m| m.as_str().parse::<i64>().ok());
        return (added, removed);
    }
    (None, None)
}

fn parse_tokens(text: &str) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    let re = match Regex::new(
        r"Usage\s*:?\s*(\d+)\s*input\s*,?\s*(\d+)\s*output\s*,?\s*(\d+)\s*cache\s*read\s*,?\s*(\d+)\s*cache\s*write",
    ) {
        Ok(r) => r,
        Err(_) => return (None, None, None, None),
    };
    if let Some(caps) = re.captures(text) {
        let i = caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok());
        let o = caps.get(2).and_then(|m| m.as_str().parse::<i64>().ok());
        let cr = caps.get(3).and_then(|m| m.as_str().parse::<i64>().ok());
        let cw = caps.get(4).and_then(|m| m.as_str().parse::<i64>().ok());
        return (i, o, cr, cw);
    }
    (None, None, None, None)
}
