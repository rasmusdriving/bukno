//! Selection as anchor and focus positions in the document, independent of
//! what is laid out. A selection can start in one message, cross code
//! blocks, and end in a block that is currently offscreen.

use super::document::{BlockId, Document};

/// A caret position: a block and a character offset in its plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPos {
    pub block: BlockId,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started. Stays put while extending.
    pub anchor: TextPos,
    /// The moving end, where the caret is.
    pub focus: TextPos,
}

impl Selection {
    pub fn caret(pos: TextPos) -> Self {
        Self { anchor: pos, focus: pos }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    /// Both ends as (block index, offset), in document order. `None` when a
    /// block no longer exists.
    pub fn ordered(&self, doc: &Document) -> Option<((usize, usize), (usize, usize))> {
        let a = resolve(doc, self.anchor)?;
        let f = resolve(doc, self.focus)?;
        Some(if a <= f { (a, f) } else { (f, a) })
    }

    /// The selected character range inside block `index`, if any.
    pub fn range_in(&self, doc: &Document, index: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered(doc)?;
        if index < start.0 || index > end.0 {
            return None;
        }
        let chars = doc.blocks[index].chars;
        let from = if index == start.0 { start.1 } else { 0 };
        let to = if index == end.0 { end.1 } else { chars };
        (from < to || (start.0 < index && index < end.0)).then_some((from, to))
    }

    pub fn text(&self, doc: &Document) -> String {
        match self.ordered(doc) {
            Some((start, end)) => doc.text_between(start, end),
            None => String::new(),
        }
    }
}

pub fn resolve(doc: &Document, pos: TextPos) -> Option<(usize, usize)> {
    let index = doc.index_of(pos.block)?;
    Some((index, pos.offset.min(doc.blocks[index].chars)))
}

pub fn pos_at(doc: &Document, index: usize, offset: usize) -> TextPos {
    let block = &doc.blocks[index];
    TextPos { block: block.id, offset: offset.min(block.chars) }
}

/// Character offset of the next word boundary, moving right.
pub fn next_word(text: &str, offset: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = offset;
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}

/// Character offset of the previous word boundary, moving left.
pub fn previous_word(text: &str, offset: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = offset.min(chars.len());
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}

/// The word around an offset, for double-click selection.
pub fn word_at(text: &str, offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut start = offset.min(chars.len());
    let mut end = start;
    while start > 0 && chars[start - 1].is_alphanumeric() {
        start -= 1;
    }
    while end < chars.len() && chars[end].is_alphanumeric() {
        end += 1;
    }
    (start, end)
}
