//! Per-document undo shared by the block, table and source editors.
use std::ops::Range;

struct Change {
    start: usize,
    removed: String,
    inserted: String,
}

pub struct History {
    previous: String,
    changes: Vec<Change>,
    next: usize,
}

impl History {
    pub fn new(text: &str) -> Self {
        Self {
            previous: text.to_owned(),
            changes: Vec::new(),
            next: 0,
        }
    }

    pub fn record(&mut self, text: &str) {
        let old = &self.previous;
        if text == old {
            return;
        }
        let start = old
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf8())
            .sum::<usize>();
        let suffix = old[start..]
            .chars()
            .rev()
            .zip(text[start..].chars().rev())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf8())
            .sum::<usize>();
        let removed = &old[start..old.len() - suffix];
        let inserted = &text[start..text.len() - suffix];
        self.changes.truncate(self.next);
        self.changes.push(Change {
            start,
            removed: removed.into(),
            inserted: inserted.into(),
        });
        if self.changes.len() > 256 {
            self.changes.remove(0);
        }
        self.next = self.changes.len();
        self.previous
            .replace_range(start..old.len() - suffix, inserted);
    }

    pub fn restore(&mut self, text: &mut String, redo: bool) -> Option<Range<usize>> {
        let index = if redo {
            self.next
        } else {
            self.next.checked_sub(1)?
        };
        let change = self.changes.get(index)?;
        let (from, to) = if redo {
            (&change.removed, &change.inserted)
        } else {
            (&change.inserted, &change.removed)
        };
        let range = change.start..change.start + from.len();
        text.replace_range(range.clone(), to);
        self.previous.replace_range(range, to);
        self.next = if redo { index + 1 } else { index };
        let caret = text[..change.start + to.len()].chars().count();
        Some(caret..caret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_across_blocks_and_branch_after_redo() {
        let source = "# 🦀\n\n- [ ] café";
        let mut text = source.to_owned();
        let mut h = History::new(&text);
        text.replace_range(2..6, "Résumé");
        h.record(&text);
        let heading = text.clone();
        text = text.replace("[ ]", "[x]");
        h.record(&text);
        h.restore(&mut text, false).unwrap();
        assert_eq!(text, heading);
        assert_eq!(h.restore(&mut text, false), Some(3..3));
        assert_eq!(text, source);
        h.restore(&mut text, true).unwrap();
        assert_eq!(text, heading);
        text.push('!');
        h.record(&text);
        assert!(h.restore(&mut text, true).is_none());
    }
}
