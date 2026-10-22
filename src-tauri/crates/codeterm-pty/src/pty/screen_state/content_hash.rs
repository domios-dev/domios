use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct ContentHash {
    rows: Vec<String>,
    prefixes: Vec<Sha256>,
    dirty: bool,
    #[cfg(test)]
    pub(super) rows_hashed: usize,
}

impl ContentHash {
    pub(super) fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub(super) fn get(&mut self, screen: &vt100::Screen, exclude_bottom_rows: u16) -> [u8; 32] {
        let (rows, cols) = screen.size();
        let end_row = rows.saturating_sub(exclude_bottom_rows) as usize;
        if end_row == 0 {
            return Sha256::new().finalize().into();
        }
        if self.dirty {
            for row in 0..self.rows.len().min(rows as usize) {
                let text = screen.contents_between(row as u16, 0, row as u16, cols);
                if self.rows[row] != text.trim() {
                    self.rows.truncate(row);
                    self.prefixes.truncate(row);
                    break;
                }
            }
            self.rows.truncate(rows as usize);
            self.prefixes.truncate(rows as usize);
            self.dirty = false;
        }
        while self.rows.len() < end_row {
            let row = self.rows.len() as u16;
            let text = screen.contents_between(row, 0, row, cols);
            let text = text.trim().to_string();
            let mut hash = self.prefixes.last().cloned().unwrap_or_default();
            if !text.is_empty() {
                hash.update(text.as_bytes());
            }
            self.rows.push(text);
            self.prefixes.push(hash);
            #[cfg(test)]
            {
                self.rows_hashed += 1;
            }
        }
        self.prefixes[end_row - 1].clone().finalize().into()
    }
}
