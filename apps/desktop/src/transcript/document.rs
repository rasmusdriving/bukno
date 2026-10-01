//! The transcript document: an ordered list of blocks with stable IDs and a
//! revision each, built from transcript items.
//!
//! Completed messages are parsed once. Only the item that is still streaming
//! is parsed again when it changes, and only its blocks get new revisions.

use std::collections::HashMap;
use std::ops::Range;

use bukno_core::ids::ItemId;
use bukno_core::message::{ItemKind, Provider, TranscriptItem};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A block's identity: its message plus its position inside that message.
/// Stable while the message streams, because blocks only grow at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId {
    pub item: ItemId,
    pub ordinal: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Agent(Provider),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    ListItem { number: Option<u64>, depth: u8 },
    Code { lang: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStyle {
    Strong,
    Emphasis,
    Code,
    Link,
}

/// Inline styling over a byte range of the block's plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: SpanStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub revision: u64,
    pub kind: BlockKind,
    /// The plain rendered text. Selection offsets count characters in it.
    pub text: String,
    pub spans: Vec<Span>,
    /// Index of the owning message in [`Document::messages`].
    pub message: usize,
    pub chars: usize,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub item: ItemId,
    pub role: Role,
    pub meta: Option<String>,
    /// Markdown source (plain text for the user), used by Copy message.
    pub source: String,
    pub first_block: usize,
    pub block_count: usize,
    pub revision: u64,
    pub completed: bool,
}

#[derive(Default)]
pub struct Document {
    pub messages: Vec<Message>,
    pub blocks: Vec<Block>,
    by_block: HashMap<BlockId, usize>,
    by_item: HashMap<ItemId, usize>,
    /// Increases whenever any block changes; the layout uses it to notice work.
    pub revision: u64,
}

impl Document {
    pub fn index_of(&self, id: BlockId) -> Option<usize> {
        self.by_block.get(&id).copied()
    }

    pub fn message_of(&self, block: usize) -> &Message {
        &self.messages[self.blocks[block].message]
    }

    /// Replace the whole conversation, such as when a chat is opened.
    pub fn load(&mut self, items: &[TranscriptItem]) {
        self.messages.clear();
        self.blocks.clear();
        self.by_block.clear();
        self.by_item.clear();
        for item in items {
            self.append(item);
        }
        self.revision += 1;
    }

    /// Insert a new item at the end, or update an existing one. Older
    /// revisions than the one already shown are ignored.
    pub fn upsert(&mut self, item: &TranscriptItem) {
        let Some(&index) = self.by_item.get(&item.id) else {
            self.append(item);
            self.revision += 1;
            return;
        };
        if item.revision <= self.messages[index].revision {
            return;
        }
        let parsed = parse(item);
        let message = &mut self.messages[index];
        message.source = item.text.clone();
        message.meta = item.meta.clone();
        message.revision = item.revision;
        message.completed = item.completed;
        let first = message.first_block;
        let old_count = message.block_count;
        message.block_count = parsed.len();

        let new_blocks: Vec<Block> = parsed
            .into_iter()
            .enumerate()
            .map(|(ordinal, (kind, text, spans))| {
                let id = BlockId { item: item.id, ordinal: ordinal as u32 };
                // Keep the old revision when nothing changed, so cached layout survives.
                let old = (ordinal < old_count).then(|| &self.blocks[first + ordinal]);
                let revision = match old {
                    Some(old) if old.kind == kind && old.text == text && old.spans == spans => old.revision,
                    Some(old) => old.revision + 1,
                    None => 1,
                };
                Block { id, revision, chars: text.chars().count(), kind, text, spans, message: index }
            })
            .collect();
        let new_count = new_blocks.len();
        self.blocks.splice(first..first + old_count, new_blocks);
        if new_count != old_count {
            // Later blocks move. Streaming happens in the last message, so
            // this usually touches nothing after it.
            for later in &mut self.messages[index + 1..] {
                later.first_block = later.first_block + new_count - old_count;
            }
            self.reindex_from(first);
        }
        self.revision += 1;
    }

    fn append(&mut self, item: &TranscriptItem) {
        let index = self.messages.len();
        let first = self.blocks.len();
        let parsed = parse(item);
        let count = parsed.len();
        for (ordinal, (kind, text, spans)) in parsed.into_iter().enumerate() {
            let id = BlockId { item: item.id, ordinal: ordinal as u32 };
            self.by_block.insert(id, self.blocks.len());
            self.blocks.push(Block { id, revision: 1, chars: text.chars().count(), kind, text, spans, message: index });
        }
        self.by_item.insert(item.id, index);
        self.messages.push(Message {
            item: item.id,
            role: match item.kind {
                ItemKind::UserMessage => Role::User,
                ItemKind::AgentMessage { provider } => Role::Agent(provider),
            },
            meta: item.meta.clone(),
            source: item.text.clone(),
            first_block: first,
            block_count: count,
            revision: item.revision,
            completed: item.completed,
        });
    }

    fn reindex_from(&mut self, from: usize) {
        self.by_block.retain(|_, i| *i < from);
        for (i, block) in self.blocks.iter().enumerate().skip(from) {
            self.by_block.insert(block.id, i);
        }
    }

    /// Plain text between two block positions, as copied. Blocks are joined
    /// by a blank line, except consecutive list items, which use one newline.
    pub fn text_between(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let mut out = String::new();
        for i in start.0..=end.0.min(self.blocks.len().saturating_sub(1)) {
            let block = &self.blocks[i];
            let from = if i == start.0 { start.1 } else { 0 };
            let to = if i == end.0 { end.1 } else { block.chars };
            if i > start.0 {
                let prev = &self.blocks[i - 1];
                let in_list = prev.message == block.message
                    && matches!(prev.kind, BlockKind::ListItem { .. })
                    && matches!(block.kind, BlockKind::ListItem { .. });
                out.push_str(if in_list { "\n" } else { "\n\n" });
            }
            out.push_str(char_slice(&block.text, from, to));
        }
        out
    }
}

pub fn char_slice(text: &str, from: usize, to: usize) -> &str {
    let start = text.char_indices().nth(from).map_or(text.len(), |(b, _)| b);
    let end = text.char_indices().nth(to).map_or(text.len(), |(b, _)| b);
    &text[start..end.max(start)]
}

type Parsed = (BlockKind, String, Vec<Span>);

fn parse(item: &TranscriptItem) -> Vec<Parsed> {
    match item.kind {
        // The user's own text is shown as typed, not interpreted as Markdown.
        ItemKind::UserMessage => vec![(BlockKind::Paragraph, item.text.clone(), Vec::new())],
        ItemKind::AgentMessage { .. } => parse_markdown(&item.text),
    }
}

/// Markdown to native blocks. HTML is shown as text and never executed;
/// images show their alt text and are never fetched.
fn parse_markdown(source: &str) -> Vec<Parsed> {
    let mut out: Vec<Parsed> = Vec::new();
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut open: Vec<(SpanStyle, usize)> = Vec::new();
    let mut kind: Option<BlockKind> = None;
    // Stack of lists: the next number for ordered lists.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut in_cell = false;

    let flush = |out: &mut Vec<Parsed>, kind: &mut Option<BlockKind>, text: &mut String, spans: &mut Vec<Span>| {
        if let Some(k) = kind.take() {
            let is_code = matches!(k, BlockKind::Code { .. });
            if is_code && text.ends_with('\n') {
                text.pop();
            }
            if !text.is_empty() || is_code {
                out.push((k, std::mem::take(text), std::mem::take(spans)));
            }
        }
        text.clear();
        spans.clear();
    };

    for event in Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Paragraph) => {
                if kind.is_none() {
                    kind = Some(BlockKind::Paragraph);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                // A paragraph inside a list item stays part of that item.
                if !matches!(kind, Some(BlockKind::ListItem { .. })) {
                    flush(&mut out, &mut kind, &mut text, &mut spans);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                kind = Some(BlockKind::Heading(heading_level(level)));
            }
            Event::End(TagEnd::Heading(_)) => flush(&mut out, &mut kind, &mut text, &mut spans),
            Event::Start(Tag::CodeBlock(code)) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                let lang = match code {
                    CodeBlockKind::Fenced(lang) => lang.split_whitespace().next().unwrap_or("").to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                kind = Some(BlockKind::Code { lang });
            }
            Event::End(TagEnd::CodeBlock) => flush(&mut out, &mut kind, &mut text, &mut spans),
            Event::Start(Tag::List(start)) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                let depth = lists.len().saturating_sub(1) as u8;
                let number = lists.last_mut().and_then(|n| {
                    let current = *n;
                    if let Some(v) = n {
                        *v += 1;
                    }
                    current
                });
                kind = Some(BlockKind::ListItem { number, depth });
            }
            Event::End(TagEnd::Item) => flush(&mut out, &mut kind, &mut text, &mut spans),
            Event::Start(Tag::TableRow | Tag::TableHead) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
                kind = Some(BlockKind::Paragraph);
                in_cell = false;
            }
            Event::End(TagEnd::TableRow | TagEnd::TableHead) => {
                flush(&mut out, &mut kind, &mut text, &mut spans);
            }
            Event::Start(Tag::TableCell) => {
                if in_cell {
                    text.push_str(" | ");
                }
                in_cell = true;
            }
            Event::Start(Tag::Strong) => open.push((SpanStyle::Strong, text.len())),
            Event::Start(Tag::Emphasis) => open.push((SpanStyle::Emphasis, text.len())),
            Event::Start(Tag::Link { .. }) => open.push((SpanStyle::Link, text.len())),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                if let Some((style, start)) = open.pop() {
                    spans.push(Span { range: start..text.len(), style });
                }
            }
            Event::Code(code) => {
                if kind.is_none() {
                    kind = Some(BlockKind::Paragraph);
                }
                let start = text.len();
                text.push_str(&code);
                spans.push(Span { range: start..text.len(), style: SpanStyle::Code });
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                if kind.is_none() {
                    kind = Some(BlockKind::Paragraph);
                }
                text.push_str(&t);
            }
            Event::SoftBreak => text.push(' '),
            Event::HardBreak => text.push('\n'),
            _ => {}
        }
    }
    flush(&mut out, &mut kind, &mut text, &mut spans);
    if out.is_empty() {
        // An empty streaming reply still has one block to hold the caret.
        out.push((BlockKind::Paragraph, String::new(), Vec::new()));
    }
    out
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
