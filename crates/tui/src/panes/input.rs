//! One line of typed text, and the keys that edit it.
//!
//! Shared by the add-watch modal (M16) and the REPL prompt (M17) because both
//! are the same thing: a line somebody is typing, which must swallow every
//! printable key rather than letting it mean what it means everywhere else.
//! Two copies of that would be two places for `q` to start quitting the TUI
//! mid-expression.
//!
//! Deliberately not a text editor. There is no cursor to move, no selection and
//! no undo: the longest thing anybody types here is an expression, and the keys
//! that would make those work are the keys the REPL needs for its history.

/// A line being typed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TextInput {
    value: String,
}

impl TextInput {
    /// Start from existing text. Only tests do — the two prompts both start
    /// empty — so it is scoped to them rather than left to look unused.
    #[cfg(test)]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    pub fn push(&mut self, c: char) {
        self.value.push(c);
    }

    /// Append pasted text, which arrives whole rather than a character at a
    /// time.
    pub fn push_str(&mut self, text: &str) {
        self.value.push_str(text);
    }

    /// Delete the last *character*, not the last byte.
    ///
    /// `String::truncate` on `len() - 1` panics on anything non-ASCII, and an
    /// expression can legitimately contain one — a string literal being
    /// compared against, most obviously.
    pub fn backspace(&mut self) {
        self.value.pop();
    }

    /// Take what was typed, leaving the line empty.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.value)
    }

    /// Replace the whole line, as `<C-p>` does when it recalls an entry.
    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    pub fn clear(&mut self) {
        self.value.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_then_taking_leaves_the_line_empty_for_the_next_one() {
        let mut input = TextInput::default();
        for c in "x + 1".chars() {
            input.push(c);
        }

        assert_eq!(input.take(), "x + 1");
        assert!(input.is_empty(), "the next expression starts from nothing");
    }

    #[test]
    fn backspace_over_a_multibyte_character_does_not_panic() {
        // `truncate(len() - 1)` would, and an expression comparing against a
        // string literal is a perfectly ordinary way to get one in here.
        let mut input = TextInput::new("name == \"café\"");
        input.backspace();
        assert_eq!(input.as_str(), "name == \"café");

        // The one that would panic: `é` is two bytes, so truncating by one
        // would land in the middle of it.
        input.backspace();
        assert_eq!(input.as_str(), "name == \"caf");
    }

    #[test]
    fn backspace_on_an_empty_line_is_a_no_op_rather_than_a_panic() {
        let mut input = TextInput::default();
        input.backspace();
        assert!(input.is_empty());
    }

    #[test]
    fn a_line_of_only_spaces_counts_as_empty_so_enter_does_not_submit_it() {
        assert!(TextInput::new("   ").is_empty());
        assert!(!TextInput::new(" x ").is_empty());
    }
}
