//! Block geometry, measured heights and cached text layout.
//!
//! Only blocks near the viewport are laid out. Every other block has an
//! estimated height until it is first shown; positions are prefix sums of
//! those heights, and the widget keeps the viewport anchored to a block so
//! a corrected estimate never makes the visible text jump.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{FontFamily, FontId, Galley, Rect, Stroke, Vec2, pos2, vec2};

use super::document::{Block, BlockId, BlockKind, Document, Role, SpanStyle};
use crate::theme::Theme;

/// Space above the first message and below the last.
pub fn top_padding(theme: &Theme) -> f32 {
    // Chat header to first message is space-8; the header already takes 8.
    theme.space.space_8 - 8.0
}
pub fn bottom_padding(theme: &Theme) -> f32 {
    theme.space.space_6
}

const USER_PAD: Vec2 = Vec2::new(16.0, 10.0);
const USER_MAX_FRACTION: f32 = 0.8;
const CODE_HEADER: f32 = 32.0;
const CODE_PAD_X: f32 = 14.0;
const CODE_PAD_TOP: f32 = 2.0;
const CODE_PAD_BOTTOM: f32 = 12.0;
const LIST_INDENT: f32 = 22.0;
const AGENT_HEADER: f32 = 20.0;
const BODY_GAP: f32 = 12.0;
const LIST_GAP: f32 = 4.0;

/// One laid-out block, positioned relative to the column's left edge and the
/// block's own top.
pub struct BlockLayout {
    pub revision: u64,
    wrap_key: u32,
    pub galley: Arc<Galley>,
    /// Total height, including the spacing above the block.
    pub height: f32,
    /// Where the galley's origin sits.
    pub text_pos: Vec2,
    /// The bubble or code frame, when the block has one.
    pub frame: Option<Rect>,
    /// Width of the visible text area, for code blocks that scroll sideways.
    pub viewport_width: f32,
}

impl BlockLayout {
    pub fn text_overflow(&self) -> f32 {
        (self.galley.size().x - self.viewport_width).max(0.0)
    }
}

/// Most laid-out blocks kept in memory. Galleys hold glyphs and meshes, so
/// keeping every block of a long chat costs far more than the 20 MiB budget.
const MAX_CACHED_LAYOUTS: usize = 400;
/// Blocks on either side of the viewport that are never dropped.
const KEEP_AROUND: usize = 60;

#[derive(Clone, Copy)]
struct Measured {
    revision: u64,
    wrap_key: u32,
    height: f32,
}

#[derive(Default)]
pub struct Layout {
    width: f32,
    pixels_per_point: f32,
    heights: Vec<f32>,
    /// Prefix sums of `heights`, plus the top padding; one longer than `heights`.
    tops: Vec<f32>,
    tops_dirty: bool,
    cache: HashMap<BlockId, BlockLayout>,
    /// Heights of blocks laid out before, kept after their galleys are
    /// dropped so returning to them never changes the scroll geometry.
    measured: HashMap<BlockId, Measured>,
    seen_revision: u64,
    pub laid_out_this_frame: usize,
    /// Space after the last block, such as for the working indicator.
    pub trailing: f32,
}

impl Layout {
    /// Bring heights in line with the document and the column width.
    pub fn sync(&mut self, doc: &Document, theme: &Theme, width: f32, pixels_per_point: f32) {
        self.laid_out_this_frame = 0;
        let width_changed = (width - self.width).abs() > 0.25;
        let fonts_changed = pixels_per_point != self.pixels_per_point;
        if fonts_changed {
            // Galleys hold glyph positions for one scale; lay everything out again.
            self.cache.clear();
        }
        if !width_changed
            && !fonts_changed
            && doc.revision == self.seen_revision
            && self.heights.len() == doc.blocks.len()
        {
            return;
        }
        self.width = width;
        self.pixels_per_point = pixels_per_point;
        self.seen_revision = doc.revision;
        self.heights.clear();
        self.heights.reserve(doc.blocks.len());
        let wrap_key = (width * 2.0).round() as u32;
        for (i, block) in doc.blocks.iter().enumerate() {
            let height = match self.measured.get(&block.id) {
                Some(m) if m.revision == block.revision && m.wrap_key == wrap_key => m.height,
                // A stale measurement is a better estimate than a guess.
                Some(m) => m.height.max(estimate(doc, i, theme, width)),
                None => estimate(doc, i, theme, width),
            };
            self.heights.push(height);
        }
        self.tops_dirty = true;
    }

    pub fn tops(&mut self, theme: &Theme) -> &[f32] {
        if self.tops_dirty || self.tops.len() != self.heights.len() + 1 {
            self.tops.clear();
            let mut y = top_padding(theme);
            self.tops.push(y);
            for h in &self.heights {
                y += h;
                self.tops.push(y);
            }
            self.tops_dirty = false;
        }
        &self.tops
    }

    pub fn total_height(&mut self, theme: &Theme) -> f32 {
        let bottom = bottom_padding(theme);
        self.tops(theme).last().copied().unwrap_or(0.0) + self.trailing + bottom
    }

    pub fn top(&mut self, theme: &Theme, index: usize) -> f32 {
        self.tops(theme)[index]
    }

    /// Index of the block that contains content position `y`.
    pub fn block_at(&mut self, theme: &Theme, y: f32) -> usize {
        let tops = self.tops(theme);
        let n = tops.len().saturating_sub(1);
        if n == 0 {
            return 0;
        }
        match tops[..n].binary_search_by(|t| t.total_cmp(&y)) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => (i - 1).min(n - 1),
        }
    }

    pub fn cached(&self, id: BlockId) -> Option<&BlockLayout> {
        self.cache.get(&id)
    }

    /// Lay out block `index` if needed. Returns true when its height changed.
    pub fn ensure(
        &mut self,
        fonts: &mut egui::epaint::text::FontsView<'_>,
        doc: &Document,
        theme: &Theme,
        index: usize,
    ) -> bool {
        let block = &doc.blocks[index];
        let wrap_key = (self.width * 2.0).round() as u32;
        if let Some(cached) = self.cache.get(&block.id)
            && cached.revision == block.revision
            && cached.wrap_key == wrap_key
        {
            if (self.heights[index] - cached.height).abs() > 0.01 {
                self.heights[index] = cached.height;
                self.tops_dirty = true;
                return true;
            }
            return false;
        }
        let laid = lay_out(fonts, doc, theme, index, self.width, wrap_key);
        self.laid_out_this_frame += 1;
        let changed = (self.heights[index] - laid.height).abs() > 0.01;
        if changed {
            self.heights[index] = laid.height;
            self.tops_dirty = true;
        }
        self.measured.insert(block.id, Measured { revision: block.revision, wrap_key, height: laid.height });
        self.cache.insert(block.id, laid);
        changed
    }

    /// Bound memory: drop galleys far from the viewport (`visible` is the
    /// range of block indices in view) and anything no longer in the document.
    pub fn trim(&mut self, doc: &Document, visible: std::ops::Range<usize>) {
        if self.cache.len() <= MAX_CACHED_LAYOUTS {
            return;
        }
        let keep = visible.start.saturating_sub(KEEP_AROUND)..visible.end + KEEP_AROUND;
        self.cache.retain(|id, _| doc.index_of(*id).is_some_and(|i| keep.contains(&i)));
        if self.measured.len() > doc.blocks.len() + 256 {
            self.measured.retain(|id, _| doc.index_of(*id).is_some());
        }
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

/// Spacing above a block: between messages, below an agent's name line,
/// or between blocks of one reply.
pub fn space_before(doc: &Document, index: usize, theme: &Theme) -> f32 {
    let block = &doc.blocks[index];
    let message = doc.message_of(index);
    let first_in_message = message.first_block == index;
    let mut space = 0.0;
    if first_in_message {
        if block.message > 0 {
            space += theme.space.space_6;
        }
        if matches!(message.role, Role::Agent(_)) {
            space += AGENT_HEADER + BODY_GAP;
        }
    } else {
        let prev = &doc.blocks[index - 1];
        let list_pair =
            matches!(prev.kind, BlockKind::ListItem { .. }) && matches!(block.kind, BlockKind::ListItem { .. });
        space += if list_pair { LIST_GAP } else { BODY_GAP };
    }
    space
}

fn style_for<'a>(theme: &'a Theme, kind: &BlockKind) -> &'a crate::theme::TypeStyle {
    match kind {
        BlockKind::Heading(_) => &theme.text.t_heading,
        BlockKind::Code { .. } => &theme.text.t_code,
        _ => &theme.text.t_body,
    }
}

fn wrap_width(doc: &Document, index: usize, width: f32) -> f32 {
    let block = &doc.blocks[index];
    match (&doc.message_of(index).role, &block.kind) {
        (Role::User, _) => width * USER_MAX_FRACTION - 2.0 * USER_PAD.x,
        (_, BlockKind::Code { .. }) => f32::INFINITY,
        (_, BlockKind::ListItem { depth, .. }) => width - LIST_INDENT * (*depth as f32 + 1.0),
        _ => width,
    }
}

/// A cheap height guess for a block that has not been laid out yet.
fn estimate(doc: &Document, index: usize, theme: &Theme, width: f32) -> f32 {
    let block = &doc.blocks[index];
    let style = style_for(theme, &block.kind);
    let lines = match block.kind {
        BlockKind::Code { .. } => block.text.lines().count().max(1) as f32,
        _ => {
            let per_line = (wrap_width(doc, index, width) / (style.size * 0.5)).max(8.0);
            block.text.split('\n').map(|l| (l.chars().count() as f32 / per_line).ceil().max(1.0)).sum()
        }
    };
    let content = lines * style.line_height;
    let chrome = match (&doc.message_of(index).role, &block.kind) {
        (Role::User, _) => 2.0 * USER_PAD.y,
        (_, BlockKind::Code { .. }) => CODE_HEADER + CODE_PAD_TOP + CODE_PAD_BOTTOM,
        _ => 0.0,
    };
    space_before(doc, index, theme) + content + chrome
}

fn lay_out(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    doc: &Document,
    theme: &Theme,
    index: usize,
    width: f32,
    wrap_key: u32,
) -> BlockLayout {
    let block = &doc.blocks[index];
    let role = doc.message_of(index).role;
    let before = space_before(doc, index, theme);
    let wrap = wrap_width(doc, index, width);
    let galley = fonts.layout_job(job(block, theme, wrap));
    let size = galley.size();

    let (text_pos, frame, height, viewport_width) = match (&role, &block.kind) {
        (Role::User, _) => {
            let bubble = vec2(size.x + 2.0 * USER_PAD.x, size.y + 2.0 * USER_PAD.y);
            let min = pos2(width - bubble.x, before);
            (min.to_vec2() + USER_PAD, Some(Rect::from_min_size(min, bubble)), before + bubble.y, size.x)
        }
        (_, BlockKind::Code { .. }) => {
            let frame = Rect::from_min_size(
                pos2(0.0, before),
                vec2(width, CODE_HEADER + CODE_PAD_TOP + size.y + CODE_PAD_BOTTOM),
            );
            (
                vec2(CODE_PAD_X, before + CODE_HEADER + CODE_PAD_TOP),
                Some(frame),
                before + frame.height(),
                width - 2.0 * CODE_PAD_X,
            )
        }
        (_, BlockKind::ListItem { depth, .. }) => {
            (vec2(LIST_INDENT * (*depth as f32 + 1.0), before), None, before + size.y, size.x)
        }
        _ => (vec2(0.0, before), None, before + size.y, size.x),
    };
    BlockLayout { revision: block.revision, wrap_key, galley, height, text_pos, frame, viewport_width }
}

/// Build the styled text for one block.
fn job(block: &Block, theme: &Theme, wrap: f32) -> LayoutJob {
    let style = style_for(theme, &block.kind);
    let base = theme.format(style, theme.color.text_primary);
    let mut job = LayoutJob { text: block.text.clone(), ..Default::default() };
    job.wrap.max_width = wrap;

    // Split the text at every span edge and combine the styles that cover each piece.
    let mut cuts: Vec<usize> = vec![0, block.text.len()];
    for span in &block.spans {
        cuts.push(span.range.start);
        cuts.push(span.range.end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start == end {
            continue;
        }
        let mut format = base.clone();
        for span in block.spans.iter().filter(|s| s.range.start <= start && s.range.end >= end) {
            apply(&mut format, span.style, theme, style);
        }
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(start)..ByteIndex(end), format });
    }
    if job.sections.is_empty() {
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(0)..ByteIndex(0), format: base });
    }
    job
}

fn apply(format: &mut TextFormat, style: SpanStyle, theme: &Theme, base: &crate::theme::TypeStyle) {
    match style {
        SpanStyle::Strong => {
            format.font_id = FontId::new(base.size, FontFamily::Name("geist-600".into()));
        }
        SpanStyle::Emphasis => format.italics = true,
        SpanStyle::Code => {
            format.font_id = FontId::new((base.size * 0.9).round(), FontFamily::Monospace);
            format.background = theme.color.surface_code;
            format.expand_bg = 1.0;
        }
        SpanStyle::Link => {
            format.underline = Stroke::new(1.0, theme.color.text_secondary);
        }
    }
}
