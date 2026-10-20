//! Char-safe splitting of preprocessed text into overlapping embedding chunks.

pub(crate) const CHUNK_SIZE: usize = 800;
pub(crate) const CHUNK_OVERLAP: usize = 100;
pub(crate) const MIN_CHUNK_LEN: usize = 50;
/// Upper bound on vectors per item, so one huge body cannot monopolise the embedder.
pub const MAX_CHUNKS_PER_ITEM: usize = 128;

/// Split text into overlapping chunks of at most `CHUNK_SIZE` chars, preferring
/// paragraph, then sentence, then word boundaries, capped at `MAX_CHUNKS_PER_ITEM`.
pub(crate) fn split_chunks(text: &str) -> Vec<String> {
    let chunks = split_uncapped(text);
    sample_evenly(chunks, MAX_CHUNKS_PER_ITEM)
}

fn split_uncapped(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= CHUNK_SIZE {
        return vec![text.to_string()];
    }

    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| p.chars().count() >= MIN_CHUNK_LEN)
        .collect();
    if paragraphs.len() > 1 && paragraphs.iter().all(|p| p.chars().count() <= CHUNK_SIZE) {
        return paragraphs.into_iter().map(String::from).collect();
    }

    let total = chars.len();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < total {
        let end = (start + CHUNK_SIZE).min(total);
        let cut = if end < total {
            start + split_point(&chars[start..end])
        } else {
            end
        };
        let chunk: String = chars[start..cut].iter().collect();
        let trimmed = chunk.trim();
        if trimmed.chars().count() >= MIN_CHUNK_LEN {
            chunks.push(trimmed.to_string());
        }
        if cut >= total {
            break;
        }
        let overlapped = cut.saturating_sub(CHUNK_OVERLAP);
        start = if overlapped > start { overlapped } else { cut };
    }
    if chunks.is_empty() {
        chunks.push(text.to_string());
    }
    chunks
}

/// Char offset to cut `window` at. Boundaries in the first half are ignored so
/// every step advances at least half a chunk.
fn split_point(window: &[char]) -> usize {
    let floor = window.len() / 2;
    let last_pair = |a: char, b: char| {
        window
            .windows(2)
            .rposition(|p| p[0] == a && p[1] == b)
            .map(|pos| pos + 2)
    };
    last_pair('\n', '\n')
        .filter(|&pos| pos > floor)
        .or_else(|| last_pair('.', ' ').filter(|&pos| pos > floor))
        .or_else(|| {
            window
                .iter()
                .rposition(|&c| c == ' ')
                .filter(|&pos| pos > floor)
        })
        .unwrap_or(window.len())
}

/// Keep `max` chunks spread across the whole body, always the first and the last.
fn sample_evenly(chunks: Vec<String>, max: usize) -> Vec<String> {
    let n = chunks.len();
    if n <= max || max < 2 {
        return chunks.into_iter().take(max.max(1)).collect();
    }
    let wanted: Vec<usize> = (0..max).map(|i| i * (n - 1) / (max - 1)).collect();
    let mut wanted = wanted.into_iter().peekable();
    chunks
        .into_iter()
        .enumerate()
        .filter_map(|(i, chunk)| {
            if wanted.peek() == Some(&i) {
                wanted.next();
                Some(chunk)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
