use std::collections::VecDeque;

/// Snapshot-based undo/redo stack.
///
/// Each entry stores the whole state before an edit. Projects are small (kilobytes), so
/// snapshots are simpler and safer than inverse commands.
#[derive(Debug, Clone)]
pub struct History<T> {
    undo: VecDeque<Entry<T>>,
    redo: Vec<Entry<T>>,
    limit: usize,
}

#[derive(Debug, Clone)]
struct Entry<T> {
    label: String,
    state: T,
}

impl<T> History<T> {
    /// Keeps at most `limit` undo steps; the oldest are dropped first.
    pub fn new(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records `before`, the state preceding an edit described by `label`.
    /// Any redo steps are discarded.
    pub fn record(&mut self, label: impl Into<String>, before: T) {
        self.undo.push_back(Entry {
            label: label.into(),
            state: before,
        });
        if self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// Restores the state before the last edit into `current`. Returns that edit's label.
    pub fn undo(&mut self, current: &mut T) -> Option<String> {
        let entry = self.undo.pop_back()?;
        let after = std::mem::replace(current, entry.state);
        self.redo.push(Entry {
            label: entry.label.clone(),
            state: after,
        });
        Some(entry.label)
    }

    /// Re-applies the last undone edit into `current`. Returns that edit's label.
    pub fn redo(&mut self, current: &mut T) -> Option<String> {
        let entry = self.redo.pop()?;
        let before = std::mem::replace(current, entry.state);
        self.undo.push_back(Entry {
            label: entry.label.clone(),
            state: before,
        });
        Some(entry.label)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_the_stack() {
        let mut h = History::new(10);
        let mut state = 0;
        for (label, value) in [("one", 1), ("two", 2)] {
            h.record(label, state);
            state = value;
        }
        assert_eq!(h.undo(&mut state).as_deref(), Some("two"));
        assert_eq!(state, 1);
        assert_eq!(h.undo(&mut state).as_deref(), Some("one"));
        assert_eq!(state, 0);
        assert_eq!(h.undo(&mut state), None);
        assert_eq!(h.redo(&mut state).as_deref(), Some("one"));
        assert_eq!(state, 1);
        assert_eq!(h.redo_label(), Some("two"));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::new(10);
        let mut state = 0;
        h.record("a", state);
        state = 1;
        h.undo(&mut state);
        h.record("b", state);
        assert!(!h.can_redo());
    }

    #[test]
    fn oldest_steps_are_dropped_at_the_limit() {
        let mut h = History::new(2);
        let mut state = 0;
        for v in 1..=3 {
            h.record(format!("set {v}"), state);
            state = v;
        }
        h.undo(&mut state);
        h.undo(&mut state);
        assert_eq!(state, 1);
        assert!(!h.can_undo());
    }
}
