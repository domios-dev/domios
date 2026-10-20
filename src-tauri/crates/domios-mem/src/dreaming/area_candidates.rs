use rusqlite::{params, Connection};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct AreaSummary {
    pub name: String,
    pub count: usize,
    pub samples: Vec<String>,
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn similar_names(a: &str, b: &str) -> bool {
    if a.starts_with('_') || b.starts_with('_') {
        return false;
    }
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    if a.len().min(b.len()) >= 3
        && (a.strip_suffix('s') == Some(b.as_str()) || b.strip_suffix('s') == Some(a.as_str()))
    {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().min(b.len()) < 6 || a.len().max(b.len()) > 128 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut next = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            next.push(
                (previous[j + 1] + 1)
                    .min(next[j] + 1)
                    .min(previous[j] + usize::from(x != y)),
            );
        }
        previous = next;
    }
    previous[b.len()] <= 1
}

pub(super) fn read_areas(conn: &Connection) -> Result<Vec<AreaSummary>, String> {
    let names: Vec<(String, usize)> = {
        let mut stmt = conn.prepare("SELECT area, COUNT(*) FROM mem_items WHERE area IS NOT NULL AND area != '' AND substr(area, 1, 1) != '_' GROUP BY area ORDER BY area").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    names
        .into_iter()
        .map(|(name, count)| {
            let mut stmt = conn
                .prepare("SELECT title FROM mem_items WHERE area=?1 ORDER BY id LIMIT 3")
                .map_err(|e| e.to_string())?;
            let samples = stmt
                .query_map(params![name], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(AreaSummary {
                name,
                count,
                samples,
            })
        })
        .collect()
}

pub(super) fn candidate_pairs(areas: &[AreaSummary]) -> Vec<(&AreaSummary, &AreaSummary)> {
    let mut pairs = Vec::new();
    for (i, a) in areas.iter().enumerate() {
        for b in &areas[i + 1..] {
            if similar_names(&a.name, &b.name) {
                pairs.push(if a.count <= b.count { (a, b) } else { (b, a) });
                if pairs.len() == 20 {
                    return pairs;
                }
            }
        }
    }
    pairs
}

pub(super) fn render_candidates(conn: &Connection) -> Result<Option<String>, String> {
    let areas = read_areas(conn)?;
    let pairs = candidate_pairs(&areas);
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut lines = vec!["\nAREA MERGE CANDIDATES:".to_string()];
    for (smaller, larger) in pairs {
        for area in [smaller, larger] {
            let samples: Vec<String> = area
                .samples
                .iter()
                .map(|s| s.chars().take(120).collect())
                .collect();
            lines.push(format!(
                "  {:?}: {} items; sample titles: {:?}",
                area.name, area.count, samples
            ));
        }
        lines.push(String::new());
    }
    Ok(Some(lines.join("\n")))
}

#[cfg(test)]
mod tests;
