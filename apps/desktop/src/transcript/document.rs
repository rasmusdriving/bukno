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

use super::selection::TextPos;

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
    /// Last block revision handed out. Block revisions only go up, even
    /// across [`Document::load`], so a rebuilt block never matches a layout
    /// cached for different text under the same block ID.
    last_block_revision: u64,
    /// Source maps of messages that are streaming, or that completed since
    /// the transcript was last drawn. A selection is made on the text as
    /// drawn, so its ends are moved through the Markdown source to the
    /// current blocks (`**bold` becoming `bold`, a paragraph becoming a
    /// table). Completed history keeps none.
    tracked: HashMap<ItemId, Tracked>,
    /// Completed messages whose maps wait for the next drawn frame, oldest first.
    settling: VecDeque<ItemId>,
}

/// For each block of a message, the byte offset in the message source of
/// each plain character, plus one end position.
type Maps = Arc<[Box<[u32]>]>;

struct Tracked {
    /// Message revision of `maps`.
    revision: u64,
    maps: Maps,
    /// The maps as last drawn, if the message has been drawn.
    drawn: Option<(u64, Maps)>,
}

/// Completed messages that may wait for a drawn frame before their maps
/// are dropped. Bounds memory even when nothing is drawn.
const SETTLING_LIMIT: usize = 4;

/// Which character a selection end belongs to when its source position
/// falls between blocks or between characters that map to one position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    /// The start of a selection, or a caret: the next character.
    Forward,
    /// The end of a selection: after the previous character.
    Backward,
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
        self.tracked.clear();
        self.settling.clear();
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
        // A user message's caption changes its height, so its blocks need a new layout.
        let caption_changed = self.messages[index].role == Role::User && self.messages[index].meta != item.meta;
        let message = &mut self.messages[index];
        message.source = item.text.clone();
        message.meta = item.meta.clone();
        message.revision = item.revision;
        message.completed = item.completed;
        let first = message.first_block;
        let old_count = message.block_count;
        message.block_count = parsed.len();

        let mut maps = Vec::with_capacity(parsed.len());
        let mut last_block_revision = self.last_block_revision;
        let new_blocks: Vec<Block> = parsed
            .into_iter()
            .enumerate()
            .map(|(ordinal, (kind, text, spans, map))| {
                let id = BlockId { item: item.id, ordinal: ordinal as u32 };
                // Keep the old revision when nothing changed, so cached layout survives.
                let old = (ordinal < old_count).then(|| &self.blocks[first + ordinal]);
                let revision = match old {
                    Some(old) if !caption_changed && old.kind == kind && old.text == text && old.spans == spans => {
                        old.revision
                    }
                    _ => {
                        last_block_revision += 1;
                        last_block_revision
                    }
                };
                if keep_maps {
                    maps.push(map.into_boxed_slice());
                }
                Block { id, revision, chars: text.chars().count(), kind, text, spans, message: index }
            })
            .collect();
        self.last_block_revision = last_block_revision;
        if keep_maps {
            let tracked =
                self.tracked.entry(item.id).or_insert(Tracked { revision: 0, maps: Maps::default(), drawn: None });
            tracked.revision = item.revision;
            tracked.maps = maps.into();
            if item.completed && !self.settling.contains(&item.id) {
                self.settling.push_back(item.id);
                while self.settling.len() > SETTLING_LIMIT {
                    let oldest = self.settling.pop_front().unwrap();
                    self.tracked.remove(&oldest);
                }
            }
        } else {
            self.tracked.remove(&item.id);
        }
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
        let mut maps = Vec::new();
        for (ordinal, (kind, text, spans, map)) in parsed.into_iter().enumerate() {
            let id = BlockId { item: item.id, ordinal: ordinal as u32 };
            if !item.completed {
                maps.push(map.into_boxed_slice());
            }
            self.by_block.insert(id, self.blocks.len());
            self.last_block_revision += 1;
            let revision = self.last_block_revision;
            self.blocks.push(Block { id, revision, chars: text.chars().count(), kind, text, spans, message: index });
        }
        if !item.completed {
            self.tracked.insert(item.id, Tracked { revision: item.revision, maps: maps.into(), drawn: None });
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

    /// Call after the transcript has been drawn. The drawn maps become the
    /// current ones, and messages that completed are released: the view has
    /// moved any selection onto their final text.
    pub fn frame_drawn(&mut self) {
        for item in self.settling.drain(..) {
            self.tracked.remove(&item);
        }
        for tracked in self.tracked.values_mut() {
            if tracked.drawn.as_ref().is_none_or(|(revision, _)| *revision != tracked.revision) {
                tracked.drawn = Some((tracked.revision, tracked.maps.clone()));
            }
        }
    }

    /// Bytes held in source maps, for the memory check.
    pub fn source_map_bytes(&self) -> usize {
        let size = |maps: &Maps| maps.iter().map(|m| m.len() * size_of::<u32>()).sum::<usize>();
        self.tracked
            .values()
            .map(|t| size(&t.maps) + t.drawn.as_ref().filter(|(r, _)| *r != t.revision).map_or(0, |(_, m)| size(m)))
            .sum()
    }

    /// Messages that currently keep source maps.
    pub fn tracked_messages(&self) -> usize {
        self.tracked.len()
    }

    /// Move a position taken on the text as last drawn to the same source
    /// text in the current blocks, which may be split or merged differently.
    /// Positions in messages that have not changed since are returned as they are.
    pub fn relocate(&self, pos: TextPos, bias: Bias) -> TextPos {
        let item = pos.block.item;
        let Some(tracked) = self.tracked.get(&item) else { return pos };
        let Some((drawn, old)) = &tracked.drawn else { return pos };
        if *drawn == tracked.revision {
            return pos;
        }
        let Some(map) = old.get(pos.block.ordinal as usize) else { return pos };
        let chars = map.len() - 1;
        let k = pos.offset.min(chars);
        let source = match bias {
            Bias::Forward => map[k],
            Bias::Backward if k > 0 => map[k - 1] + 1,
            Bias::Backward => map[0],
        };
        let block = |ordinal: usize| BlockId { item, ordinal: ordinal as u32 };
        let maps = &tracked.maps;
        match bias {
            // The first character at or after the source position.
            Bias::Forward => {
                for (ordinal, m) in maps.iter().enumerate() {
                    let n = m.len() - 1;
                    let i = m[..n].partition_point(|&s| s < source);
                    if i < n {
                        return TextPos { block: block(ordinal), offset: i };
                    }
                }
                let last = maps.len().saturating_sub(1);
                TextPos { block: block(last), offset: maps.get(last).map_or(0, |m| m.len() - 1) }
            }
            // Just after the last character before the source position.
            Bias::Backward => {
                for (ordinal, m) in maps.iter().enumerate().rev() {
                    let n = m.len() - 1;
                    let i = m[..n].partition_point(|&s| s < source);
                    if i > 0 {
                        return TextPos { block: block(ordinal), offset: i };
                    }
                }
                TextPos { block: block(0), offset: 0 }
            }
        }
    }

    /// True when `a` comes before or at `b`, using the blocks as drawn.
    pub fn precedes(&self, a: TextPos, b: TextPos) -> bool {
        if a.block.item == b.block.item {
            return (a.block.ordinal, a.offset) <= (b.block.ordinal, b.offset);
        }
        self.by_item.get(&a.block.item) <= self.by_item.get(&b.block.item)
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

    /// Append rendered text that came from `range` of the source. Each
    /// character maps to its own source position. Where the rendering differs
    /// from the source (escapes, entities, code spans that join lines), each
    /// character maps to the next source character that renders the same.
    fn push(&mut self, rendered: &str, source: &str, range: Range<usize>) {
        self.begin(BlockKind::Paragraph);
        let slice = source.get(range.clone()).unwrap_or("");
        // Reserve the whole piece, so retained text does not keep doubling slack.
        self.text.reserve(rendered.len());
        if slice == rendered {
            for (b, ch) in rendered.char_indices() {
                self.text.push(ch);
                self.map.push((range.start + b) as u32);
            }
        } else {
            let mut cursor = 0;
            for ch in rendered.chars() {
                let found = slice[cursor..].char_indices().find(|&(_, s)| s == ch || (ch == ' ' && s == '\n'));
                let at = match found {
                    Some((i, s)) => {
                        let at = cursor + i;
                        cursor = at + s.len_utf8();
                        at
                    }
                    None => cursor,
                };
                self.text.push(ch);
                self.map.push((range.start + at) as u32);
            }
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
