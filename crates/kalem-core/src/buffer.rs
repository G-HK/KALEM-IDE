/// Minimal text buffer placeholder.
///
/// This is a plain `String` for now. The planned replacement is a rope
/// (e.g. `ropey`) with an undo tree and multi-cursor support; keeping the API
/// tiny means callers will not need to change.
#[derive(Debug, Default, Clone)]
pub struct Buffer {
    text: String,
}

impl Buffer {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn len_lines(&self) -> usize {
        if self.text.is_empty() {
            1
        } else {
            self.text.split('\n').count()
        }
    }

    /// Insert `s` at byte offset `at` (must be on a char boundary).
    pub fn insert(&mut self, at: usize, s: &str) {
        self.text.insert_str(at, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_and_inserts() {
        let mut b = Buffer::from_text("a\nb");
        assert_eq!(b.len_lines(), 2);
        b.insert(1, "x");
        assert_eq!(b.as_str(), "ax\nb");
        assert_eq!(Buffer::default().len_lines(), 1);
    }
}
