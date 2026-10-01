//! The transcript document: an ordered list of blocks with stable IDs and a
//! revision each, built from transcript items.
//!
//! Completed messages are parsed once. Only the item that is still streaming
//! is parsed again when it changes, and only its blocks get new revisions.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;

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
    /// Recent source maps of blocks in messages that are still streaming,
    /// by block revision. Used to keep a selection on the same source text
    /// when finishing Markdown changes the rendered text (`**bold` becoming
    /// `bold`). Completed history keeps none.
    source_maps: HashMap<BlockId, VecDeque<(u64, SourceMap)>>,
}

/// Byte offset in the message source of each plain character, plus one end position.
type SourceMap = Arc<[u32]>;

/// Source-map revisions kept per streaming block. Several revisions can
/// arrive between two frames; the view remaps at least once a frame.
const MAP_HISTORY: usize = 32;

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
        self.source_maps.clear();
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
        let keep_maps = !self.messages[index].completed || !item.completed;
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
            .map(|(ordinal, (kind, text, spans, map))| {
                let id = BlockId { item: item.id, ordinal: ordinal as u32 };
                // Keep the old revision when nothing changed, so cached layout survives.
                let old = (ordinal < old_count).then(|| &self.blocks[first + ordinal]);
                let revision = match old {
                    Some(old) if old.kind == kind && old.text == text && old.spans == spans => old.revision,
                    Some(old) => old.revision + 1,
                    None => 1,
                };
                if keep_maps {
                    let history = self.source_maps.entry(id).or_default();
                    if history.back().is_none_or(|(r, _)| *r != revision) {
                        history.push_back((revision, map.into()));
                        if history.len() > MAP_HISTORY {
                            history.pop_front();
                        }
                    }
                }
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
        for (ordinal, (kind, text, spans, map)) in parsed.into_iter().enumerate() {
            let id = BlockId { item: item.id, ordinal: ordinal as u32 };
            if !item.completed {
                self.source_maps.entry(id).or_default().push_back((1, map.into()));
            }
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

    /// Current revisions of every block that keeps source maps.
    pub fn tracked_revisions(&self) -> impl Iterator<Item = (BlockId, u64)> + '_ {
        self.source_maps.keys().filter_map(|id| Some((*id, self.blocks[self.index_of(*id)?].revision)))
    }

    /// True when the block keeps source maps (its message is streaming or
    /// streamed in this session).
    pub fn tracks_source(&self, id: BlockId) -> bool {
        self.source_maps.contains_key(&id)
    }

    /// Move a character offset taken at block revision `from` to the same
    /// source position in the block's current text. Offsets that cannot be
    /// mapped are clamped.
    pub fn remap(&self, id: BlockId, from: u64, offset: usize) -> usize {
        let Some(index) = self.index_of(id) else { return offset };
        let block = &self.blocks[index];
        if from == block.revision {
            return offset.min(block.chars);
        }
        let Some(history) = self.source_maps.get(&id) else { return offset.min(block.chars) };
        let find = |rev: u64| history.iter().rev().find(|(r, _)| *r == rev).map(|(_, m)| m.clone());
        let (Some(old), Some(new)) = (find(from), find(block.revision)) else {
            return offset.min(block.chars);
        };
        let source = old[offset.min(old.len() - 1)];
        new.partition_point(|&s| s < source).min(block.chars)
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

/// One parsed block: kind, plain text, inline spans, and for every plain
/// character (plus one end position) its byte offset in the message source.
type Parsed = (BlockKind, String, Vec<Span>, Vec<u32>);

fn parse(item: &TranscriptItem) -> Vec<Parsed> {
    match item.kind {
        // The user's own text is shown as typed, not interpreted as Markdown.
        ItemKind::UserMessage => {
            let mut map: Vec<u32> = item.text.char_indices().map(|(b, _)| b as u32).collect();
            map.push(item.text.len() as u32);
            vec![(BlockKind::Paragraph, item.text.clone(), Vec::new(), map)]
        }
        ItemKind::AgentMessage { .. } => parse_markdown(&item.text),
    }
}

/// Collects one block's text together with its source map.
#[derive(Default)]
struct BlockBuilder {
    kind: Option<BlockKind>,
    text: String,
    spans: Vec<Span>,
    map: Vec<u32>,
    /// Source position just after the last text taken.
    end: u32,
}

impl BlockBuilder {
    fn begin(&mut self, kind: BlockKind) {
        if self.kind.is_none() {
            self.kind = Some(kind);
        }
    }

    /// Append rendered text that came from `range` of the source. When the
    /// rendered text matches the source exactly each character maps to its
    /// own position; otherwise (escapes, entities) all map to the start.
    fn push(&mut self, rendered: &str, source: &str, range: Range<usize>) {
        self.begin(BlockKind::Paragraph);
        let exact = source.get(range.clone()) == Some(rendered);
        let base = source.get(range.clone()).and_then(|s| s.find(rendered)).filter(|_| !exact).map(|i| range.start + i);
        for (b, ch) in rendered.char_indices() {
            self.text.push(ch);
            let at = if exact {
                range.start + b
            } else if let Some(base) = base {
                base + b
            } else {
                range.start
            };
            self.map.push(at as u32);
        }
        self.end = self.end.max(range.end as u32);
    }

    /// Text Bukno adds itself, such as a table cell separator.
    fn push_synthetic(&mut self, rendered: &str) {
        self.begin(BlockKind::Paragraph);
        for ch in rendered.chars() {
            self.text.push(ch);
            self.map.push(self.end);
        }
    }

    fn flush(&mut self, out: &mut Vec<Parsed>) {
        if let Some(kind) = self.kind.take() {
            let is_code = matches!(kind, BlockKind::Code { .. });
            if is_code && self.text.ends_with('\n') {
                self.text.pop();
                self.map.pop();
            }
            if !self.text.is_empty() || is_code {
                let mut map = std::mem::take(&mut self.map);
                map.push(self.end);
                out.push((kind, std::mem::take(&mut self.text), std::mem::take(&mut self.spans), map));
            }
        }
        self.text.clear();
        self.spans.clear();
        self.map.clear();
    }
}

/// Markdown to native blocks. HTML is shown as text and never executed;
/// images show their alt text and are never fetched.
fn parse_markdown(source: &str) -> Vec<Parsed> {
    let mut out: Vec<Parsed> = Vec::new();
    let mut b = BlockBuilder::default();
    let mut open: Vec<(SpanStyle, usize)> = Vec::new();
    // Stack of lists: the next number for ordered lists.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut in_cell = false;

    let parser = Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Paragraph) => b.begin(BlockKind::Paragraph),
            Event::End(TagEnd::Paragraph) => {
                // A paragraph inside a list item stays part of that item.
                if !matches!(b.kind, Some(BlockKind::ListItem { .. })) {
                    b.flush(&mut out);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                b.flush(&mut out);
                b.kind = Some(BlockKind::Heading(heading_level(level)));
            }
            Event::End(TagEnd::Heading(_)) => b.flush(&mut out),
            Event::Start(Tag::CodeBlock(code)) => {
                b.flush(&mut out);
                let lang = match code {
                    CodeBlockKind::Fenced(lang) => lang.split_whitespace().next().unwrap_or("").to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                b.kind = Some(BlockKind::Code { lang });
            }
            Event::End(TagEnd::CodeBlock) => b.flush(&mut out),
            Event::Start(Tag::List(start)) => {
                b.flush(&mut out);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                b.flush(&mut out);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                b.flush(&mut out);
                let depth = lists.len().saturating_sub(1) as u8;
                let number = lists.last_mut().and_then(|n| {
                    let current = *n;
                    if let Some(v) = n {
                        *v += 1;
                    }
                    current
                });
                b.kind = Some(BlockKind::ListItem { number, depth });
            }
            Event::End(TagEnd::Item) => b.flush(&mut out),
            Event::Start(Tag::TableRow | Tag::TableHead) => {
                b.flush(&mut out);
                b.kind = Some(BlockKind::Paragraph);
                in_cell = false;
            }
            Event::End(TagEnd::TableRow | TagEnd::TableHead) => b.flush(&mut out),
            Event::Start(Tag::TableCell) => {
                if in_cell {
                    b.push_synthetic(" | ");
                }
                in_cell = true;
            }
            Event::Start(Tag::Strong) => open.push((SpanStyle::Strong, b.text.len())),
            Event::Start(Tag::Emphasis) => open.push((SpanStyle::Emphasis, b.text.len())),
            Event::Start(Tag::Link { .. }) => open.push((SpanStyle::Link, b.text.len())),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                if let Some((style, start)) = open.pop() {
                    b.spans.push(Span { range: start..b.text.len(), style });
                }
            }
            Event::Code(code) => {
                let start = b.text.len();
                b.push(&code, source, range);
                b.spans.push(Span { range: start..b.text.len(), style: SpanStyle::Code });
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => b.push(&t, source, range),
            Event::SoftBreak => b.push(" ", source, range.start..range.start),
            Event::HardBreak => b.push("\n", source, range.start..range.start),
            _ => {}
        }
    }
    b.flush(&mut out);
    if out.is_empty() {
        // An empty streaming reply still has one block to hold the caret.
        out.push((BlockKind::Paragraph, String::new(), Vec::new(), vec![0]));
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
