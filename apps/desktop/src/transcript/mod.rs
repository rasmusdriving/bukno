//! The one custom transcript widget (specification section 13).
//!
//! The widget owns scrolling, selection and accessibility for the whole
//! conversation. It lays out and paints only the blocks near the viewport,
//! while the selection lives in document coordinates, so selecting from one
//! message to another far below survives scrolling either end out of view.

pub mod accessibility;
pub mod document;
pub mod layout;
pub mod selection;

use std::collections::HashMap;

use egui::text::{CCursor, CharIndex};
use egui::{Align2, Event, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Ui, pos2, vec2};

use self::document::{BlockId, BlockKind, Document, Role};
use self::layout::{Layout, bottom_padding};
use self::selection::{Selection, TextPos, next_word, pos_at, previous_word, resolve, word_at};
use crate::components::icons;
use crate::theme::Theme;

pub const TRANSCRIPT_ID: &str = "bukno-transcript";
/// Extra height laid out above and below the viewport.
const OVERSCAN: f32 = 240.0;
/// Side gutter between the column and the canvas edge on narrow windows.
pub const GUTTER: f32 = 24.0;

pub fn transcript_id() -> Id {
    Id::new(TRANSCRIPT_ID)
}

pub struct TranscriptOutput {
    pub response: Response,
    /// Where the working indicator goes, when space was reserved and it is in view.
    pub trailing: Option<Rect>,
}

/// Per-frame numbers for the performance checks.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub visible_blocks: usize,
    pub laid_out: usize,
    pub cached: usize,
}

pub struct TranscriptView {
    layout: Layout,
    pub selection: Option<Selection>,
    /// Content position at the top of the viewport.
    offset: f32,
    /// Keep the end of the conversation in view as output arrives.
    follow: bool,
    unseen_output: bool,
    end_signature: (usize, u64),
    dragging: bool,
    /// The caret is drawn after keyboard use, not after a click.
    keyboard_caret: bool,
    reveal_caret: bool,
    /// Horizontal position kept while moving up and down, in column points.
    preferred_x: Option<f32>,
    code_scroll: HashMap<BlockId, f32>,
    viewport_height: f32,
    pub stats: FrameStats,
    /// Last text this widget copied, kept for UI checks.
    pub last_copied: Option<String>,
}

impl Default for TranscriptView {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            selection: None,
            offset: 0.0,
            follow: true,
            unseen_output: false,
            end_signature: (0, 0),
            dragging: false,
            keyboard_caret: false,
            reveal_caret: false,
            preferred_x: None,
            code_scroll: HashMap::new(),
            viewport_height: 0.0,
            stats: FrameStats::default(),
            last_copied: None,
        }
    }
}

impl TranscriptView {
    /// Forget scroll and selection, for example when another chat opens.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Scroll so that block `index` is at the top of the viewport.
    pub fn scroll_to_block(&mut self, theme: &Theme, index: usize) {
        self.offset = self.layout.top(theme, index);
        self.follow = false;
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.offset = (self.offset + dy).max(0.0);
        self.follow = false;
    }

    pub fn scroll_to_end(&mut self) {
        self.follow = true;
        self.unseen_output = false;
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &Document, theme: &Theme, trailing: f32) -> TranscriptOutput {
        self.layout.trailing = if trailing > 0.0 { trailing + theme.space.space_6 } else { 0.0 };
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let id = transcript_id();
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let column = column_rect(rect, theme);
        let ctx = ui.ctx().clone();
        self.viewport_height = rect.height();
        self.layout.sync(doc, theme, column.width(), ctx.pixels_per_point());
        self.layout.retain(doc);
        if let Some(sel) = self.selection
            && sel.ordered(doc).is_none()
        {
            self.selection = None;
        }

        let end = (doc.blocks.len(), doc.blocks.last().map_or(0, |b| b.revision));
        if end != self.end_signature && !self.follow && self.end_signature != (0, 0) {
            self.unseen_output = true;
        }
        self.end_signature = end;

        let has_focus = response.has_focus();
        if has_focus {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }
        if response.gained_focus() && self.selection.is_none() {
            // Tabbing in puts the caret at the start of the first block in view.
            let first = self.layout.block_at(theme, self.offset);
            if !doc.blocks.is_empty() {
                self.selection = Some(Selection::caret(pos_at(doc, first, 0)));
            }
            self.keyboard_caret = true;
        }

        // Wheel and trackpad scrolling.
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta.y != 0.0 {
                self.offset -= delta.y;
                self.follow = false;
            }
            if delta.x != 0.0
                && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
            {
                self.scroll_code_at(doc, theme, rect, column, pointer, -delta.x);
            }
        }

        if has_focus {
            self.handle_keys(ui, doc, theme, rect);
        }

        self.place(&ctx, doc, theme, rect.height());
        if self.follow {
            self.unseen_output = false;
        }

        self.handle_pointer(ui, &response, doc, theme, rect, column);

        // Visible range, laid out by place().
        let first = self.layout.block_at(theme, (self.offset - OVERSCAN).max(0.0));
        let last = self.layout.block_at(theme, self.offset + rect.height() + OVERSCAN);
        let visible = if doc.blocks.is_empty() { 0..0 } else { first..last + 1 };
        ctx.fonts_mut(|fonts| {
            for i in visible.clone() {
                self.layout.ensure(fonts, doc, theme, i);
            }
        });
        self.stats = FrameStats {
            visible_blocks: visible.len(),
            laid_out: self.layout.laid_out_this_frame,
            cached: self.layout.cache_len(),
        };

        let painter = ui.painter_at(rect);
        let mut positions = Vec::with_capacity(visible.len());
        for i in visible.clone() {
            let top = rect.top() - self.offset + self.layout.top(theme, i);
            let origin = pos2(column.left(), top);
            positions.push((i, origin));
            self.paint_block(ui, &painter, doc, theme, column, i, origin, has_focus);
        }

        if ctx.accesskit_node_builder(id, |_| ()).is_some() {
            accessibility::publish(ui, id, doc, &self.layout, &positions, self.selection, &self.code_scroll);
        }

        self.paint_scrollbar(ui, theme, rect);
        if self.unseen_output {
            let button = Rect::from_center_size(pos2(column.center().x, rect.bottom() - 28.0), vec2(124.0, 32.0));
            if crate::components::raised_button(ui, theme, button, "New output", "Jump to new output").clicked() {
                self.scroll_to_end();
            }
        }
        let trailing = (trailing > 0.0).then(|| {
            let end = self.layout.top(theme, doc.blocks.len()) + theme.space.space_6;
            Rect::from_min_size(pos2(column.left(), rect.top() - self.offset + end), vec2(column.width(), trailing))
        });
        TranscriptOutput { response, trailing: trailing.filter(|r| r.intersects(rect)) }
    }

    /// Lay out what the viewport needs and settle the scroll offset, keeping
    /// the first visible block where it was on screen.
    fn place(&mut self, ctx: &egui::Context, doc: &Document, theme: &Theme, height: f32) {
        let n = doc.blocks.len();
        if n == 0 {
            self.offset = 0.0;
            return;
        }
        if self.reveal_caret {
            self.reveal_caret = false;
            self.reveal(ctx, doc, theme, height);
        }
        ctx.fonts_mut(|fonts| {
            if self.follow {
                let mut covered = bottom_padding(theme);
                let mut i = n;
                while i > 0 && covered < height + OVERSCAN {
                    i -= 1;
                    self.layout.ensure(fonts, doc, theme, i);
                    covered += self.layout.top(theme, i + 1) - self.layout.top(theme, i);
                }
                self.offset = (self.layout.total_height(theme) - height).max(0.0);
                return;
            }
            self.offset = self.offset.max(0.0);
            let anchor = self.layout.block_at(theme, self.offset);
            let delta = self.offset - self.layout.top(theme, anchor);
            for i in anchor.saturating_sub(2)..anchor {
                self.layout.ensure(fonts, doc, theme, i);
            }
            let mut i = anchor;
            while i < n && self.layout.top(theme, i) - self.layout.top(theme, anchor) - delta < height + OVERSCAN {
                self.layout.ensure(fonts, doc, theme, i);
                i += 1;
            }
            let max = (self.layout.total_height(theme) - height).max(0.0);
            self.offset = (self.layout.top(theme, anchor) + delta).clamp(0.0, max);
            if self.offset >= max - 0.5 && !self.dragging {
                self.follow = true;
            }
        });
    }

    /// Scroll just enough to show the caret.
    fn reveal(&mut self, ctx: &egui::Context, doc: &Document, theme: &Theme, height: f32) {
        let Some(sel) = self.selection else { return };
        let Some((index, offset)) = resolve(doc, sel.focus) else { return };
        ctx.fonts_mut(|fonts| self.layout.ensure(fonts, doc, theme, index));
        let block_top = self.layout.top(theme, index);
        let Some(laid) = self.layout.cached(doc.blocks[index].id) else { return };
        let caret = laid.galley.pos_from_cursor(CCursor::new(offset));
        let top = block_top + laid.text_pos.y + caret.min.y;
        let bottom = top + caret.height();
        if top < self.offset + 8.0 {
            self.offset = (top - 8.0).max(0.0);
            self.follow = false;
        } else if bottom > self.offset + height - 8.0 {
            self.offset = bottom - height + 8.0;
            self.follow = false;
        }
    }

    fn scroll_code_at(&mut self, doc: &Document, theme: &Theme, rect: Rect, column: Rect, pointer: Pos2, dx: f32) {
        let y = pointer.y - rect.top() + self.offset;
        let index = self.layout.block_at(theme, y);
        let Some(block) = doc.blocks.get(index) else { return };
        if !matches!(block.kind, BlockKind::Code { .. }) {
            return;
        }
        if let Some(laid) = self.layout.cached(block.id) {
            let max = laid.text_overflow();
            let scroll = self.code_scroll.entry(block.id).or_default();
            *scroll = (*scroll + dx).clamp(0.0, max);
        }
        let _ = column;
    }

    /// Document position under a screen point. Points above or below the
    /// laid-out blocks clamp to the nearest one.
    fn hit(&mut self, doc: &Document, theme: &Theme, rect: Rect, column: Rect, pointer: Pos2) -> Option<TextPos> {
        if doc.blocks.is_empty() {
            return None;
        }
        let y = (pointer.y.clamp(rect.top(), rect.bottom()) - rect.top() + self.offset).max(0.0);
        let index = self.layout.block_at(theme, y);
        let block = &doc.blocks[index];
        let block_top = self.layout.top(theme, index);
        let laid = self.layout.cached(block.id)?;
        let origin = pos2(column.left(), rect.top() - self.offset + block_top);
        let scroll = self.code_scroll.get(&block.id).copied().unwrap_or(0.0);
        let local = pointer - origin - laid.text_pos + vec2(scroll, 0.0);
        let local = vec2(local.x, local.y.max(0.0));
        let cursor = laid.galley.cursor_from_pos(local);
        Some(pos_at(doc, index, cursor.index.0))
    }

    fn handle_pointer(
        &mut self,
        ui: &mut Ui,
        response: &Response,
        doc: &Document,
        theme: &Theme,
        rect: Rect,
        column: Rect,
    ) {
        let (pressed, down, pos, shift, clicks) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
                i.modifiers.shift,
                i.pointer
                    .button_triple_clicked(egui::PointerButton::Primary)
                    .then_some(3)
                    .or_else(|| i.pointer.button_double_clicked(egui::PointerButton::Primary).then_some(2)),
            )
        });
        let Some(pos) = pos else { return };

        if pressed && response.hovered() {
            response.request_focus();
            self.keyboard_caret = false;
            self.preferred_x = None;
            if let Some(hit) = self.hit(doc, theme, rect, column, pos) {
                self.selection = Some(match (shift, self.selection) {
                    (true, Some(sel)) => Selection { focus: hit, ..sel },
                    _ => Selection::caret(hit),
                });
                self.dragging = true;
            }
        }
        if self.dragging {
            if !down {
                self.dragging = false;
            } else {
                // Drag past the edge to scroll; the selection keeps growing.
                let edge = if pos.y < rect.top() + 12.0 {
                    pos.y - (rect.top() + 12.0)
                } else if pos.y > rect.bottom() - 12.0 {
                    pos.y - (rect.bottom() - 12.0)
                } else {
                    0.0
                };
                if edge != 0.0 {
                    let dt = ui.input(|i| i.stable_dt).min(0.05);
                    self.offset = (self.offset + edge * 18.0 * dt).max(0.0);
                    self.follow = false;
                    ui.ctx().request_repaint();
                }
                if let (Some(hit), Some(sel)) = (self.hit(doc, theme, rect, column, pos), self.selection.as_mut()) {
                    sel.focus = hit;
                }
            }
        }
        if let (Some(clicks), true) = (clicks, response.hovered())
            && let Some(sel) = self.selection
            && let Some((index, offset)) = resolve(doc, sel.focus)
        {
            let block = &doc.blocks[index];
            let (start, end) = if clicks == 3 { (0, block.chars) } else { word_at(&block.text, offset) };
            self.selection = Some(Selection { anchor: pos_at(doc, index, start), focus: pos_at(doc, index, end) });
            self.dragging = false;
        }
    }

    fn handle_keys(&mut self, ui: &mut Ui, doc: &Document, theme: &Theme, rect: Rect) {
        if doc.blocks.is_empty() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        let mut copied = false;
        for event in events {
            match event {
                Event::Copy => {
                    if !copied {
                        copied = true;
                        self.copy(ui, doc);
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.command && key == Key::C {
                        if modifiers.shift {
                            self.copy_message(ui, doc);
                        } else if !copied {
                            copied = true;
                            self.copy(ui, doc);
                        }
                        continue;
                    }
                    if modifiers.command && key == Key::A {
                        let last = doc.blocks.len() - 1;
                        self.selection = Some(Selection {
                            anchor: pos_at(doc, 0, 0),
                            focus: pos_at(doc, last, doc.blocks[last].chars),
                        });
                        continue;
                    }
                    if key == Key::Escape {
                        if let Some(sel) = self.selection.as_mut() {
                            sel.anchor = sel.focus;
                        }
                        continue;
                    }
                    if key == Key::PageUp || key == Key::PageDown {
                        let page = rect.height() * 0.9;
                        self.scroll_by(if key == Key::PageUp { -page } else { page });
                        continue;
                    }
                    if let Some(target) = self.motion(ui, doc, theme, key, modifiers) {
                        let sel = self.selection.unwrap_or(Selection::caret(target));
                        self.selection = Some(if modifiers.shift {
                            Selection { focus: target, ..sel }
                        } else {
                            Selection::caret(target)
                        });
                        self.keyboard_caret = true;
                        self.reveal_caret = true;
                        ui.ctx().request_repaint();
                    }
                }
                _ => {}
            }
        }
    }

    /// Where a navigation key moves the caret.
    fn motion(&mut self, ui: &Ui, doc: &Document, theme: &Theme, key: Key, m: Modifiers) -> Option<TextPos> {
        let sel = self.selection?;
        let (index, offset) = resolve(doc, sel.focus)?;
        let last = doc.blocks.len() - 1;
        let block = &doc.blocks[index];
        if !matches!(key, Key::ArrowUp | Key::ArrowDown) || m.alt || m.command {
            self.preferred_x = None;
        }
        // Without Shift, Left and Right first collapse a selection to its edge.
        if !m.shift && !sel.is_empty() && matches!(key, Key::ArrowLeft | Key::ArrowRight) && !m.alt && !m.command {
            let (start, end) = sel.ordered(doc)?;
            let (i, o) = if key == Key::ArrowLeft { start } else { end };
            return Some(pos_at(doc, i, o));
        }
        let pos = |i: usize, o: usize| Some(pos_at(doc, i, o));
        match key {
            Key::ArrowLeft if m.command => {
                let laid = self.layout.cached(block.id)?;
                pos(index, laid.galley.cursor_begin_of_row(&CCursor::new(offset)).index.0)
            }
            Key::ArrowRight if m.command => {
                let laid = self.layout.cached(block.id)?;
                pos(index, laid.galley.cursor_end_of_row(&CCursor::new(offset)).index.0)
            }
            Key::ArrowLeft if m.alt => {
                if offset == 0 && index > 0 {
                    let prev = &doc.blocks[index - 1];
                    pos(index - 1, previous_word(&prev.text, prev.chars))
                } else {
                    pos(index, previous_word(&block.text, offset))
                }
            }
            Key::ArrowRight if m.alt => {
                if offset >= block.chars && index < last {
                    pos(index + 1, next_word(&doc.blocks[index + 1].text, 0))
                } else {
                    pos(index, next_word(&block.text, offset))
                }
            }
            Key::ArrowLeft => match offset {
                0 if index > 0 => pos(index - 1, doc.blocks[index - 1].chars),
                0 => pos(0, 0),
                _ => pos(index, offset - 1),
            },
            Key::ArrowRight => {
                if offset >= block.chars && index < last {
                    pos(index + 1, 0)
                } else {
                    pos(index, offset + 1)
                }
            }
            Key::ArrowUp | Key::Home if m.command || key == Key::Home => pos(0, 0),
            Key::ArrowDown | Key::End if m.command || key == Key::End => pos(last, doc.blocks[last].chars),
            Key::ArrowUp if m.alt => {
                if offset == 0 && index > 0 {
                    pos(index - 1, 0)
                } else {
                    pos(index, 0)
                }
            }
            Key::ArrowDown if m.alt => {
                if offset >= block.chars && index < last {
                    pos(index + 1, doc.blocks[index + 1].chars)
                } else {
                    pos(index, block.chars)
                }
            }
            Key::ArrowUp | Key::ArrowDown => self.vertical(ui, doc, theme, index, offset, key == Key::ArrowUp),
            _ => None,
        }
    }

    /// Move one visual row up or down, crossing into the neighbouring block.
    fn vertical(
        &mut self,
        ui: &Ui,
        doc: &Document,
        theme: &Theme,
        index: usize,
        offset: usize,
        up: bool,
    ) -> Option<TextPos> {
        let last = doc.blocks.len() - 1;
        let ctx = ui.ctx().clone();
        ctx.fonts_mut(|fonts| {
            self.layout.ensure(fonts, doc, theme, index);
            if index > 0 {
                self.layout.ensure(fonts, doc, theme, index - 1);
            }
            if index < last {
                self.layout.ensure(fonts, doc, theme, index + 1);
            }
        });
        let laid = self.layout.cached(doc.blocks[index].id)?;
        let galley = laid.galley.clone();
        let here = galley.pos_from_cursor(CCursor::new(offset));
        let x = *self.preferred_x.get_or_insert(laid.text_pos.x + here.center().x);
        let row = galley.layout_from_cursor(CCursor::new(offset)).row;
        let target_row = if up { row.checked_sub(1) } else { (row + 1 < galley.rows.len()).then_some(row + 1) };
        if let Some(target) = target_row {
            let r = &galley.rows[target];
            let local = vec2(x - laid.text_pos.x, r.pos.y + r.height() / 2.0);
            return Some(pos_at(doc, index, galley.cursor_from_pos(local).index.0));
        }
        let next = if up { index.checked_sub(1)? } else { (index < last).then_some(index + 1)? };
        let other = self.layout.cached(doc.blocks[next].id)?;
        let rows = &other.galley.rows;
        let r = if up { rows.last()? } else { rows.first()? };
        let local = vec2(x - other.text_pos.x, r.pos.y + r.height() / 2.0);
        Some(pos_at(doc, next, other.galley.cursor_from_pos(local).index.0))
    }

    fn copy(&mut self, ui: &Ui, doc: &Document) {
        let Some(sel) = self.selection.filter(|s| !s.is_empty()) else { return };
        let text = sel.text(doc);
        ui.ctx().copy_text(text.clone());
        self.last_copied = Some(text);
    }

    /// Copy the Markdown source of the message that holds the caret.
    fn copy_message(&mut self, ui: &Ui, doc: &Document) {
        let Some((index, _)) = self.selection.and_then(|s| resolve(doc, s.focus)) else { return };
        let source = doc.message_of(index).source.clone();
        ui.ctx().copy_text(source.clone());
        self.last_copied = Some(source);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_block(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        doc: &Document,
        theme: &Theme,
        column: Rect,
        index: usize,
        origin: Pos2,
        focused: bool,
    ) {
        let block = &doc.blocks[index];
        let message = doc.message_of(index);
        let Some(laid) = self.layout.cached(block.id) else { return };
        let c = &theme.color;
        let text_origin = origin + laid.text_pos;

        // The agent's name line, above its first block.
        if let Role::Agent(provider) = message.role
            && message.first_block == index
        {
            let y = text_origin.y - 32.0;
            let name = crate::components::provider_name(provider);
            let galley = painter.layout_job(theme.job(
                name,
                &theme.text.t_ui_strong,
                crate::components::provider_color(theme, provider),
                f32::INFINITY,
            ));
            let name_w = galley.size().x;
            painter.galley(pos2(origin.x, y), galley, c.text_primary);
            if let Some(meta) = &message.meta {
                let galley =
                    painter.layout_job(theme.job(meta.as_str(), &theme.text.t_small, c.text_tertiary, f32::INFINITY));
                painter.galley(pos2(origin.x + name_w + 8.0, y + 1.0), galley, c.text_tertiary);
            }
        }

        // Copy message: shown while the message is hovered or holds the caret.
        let caret_here = self
            .selection
            .and_then(|s| resolve(doc, s.focus))
            .is_some_and(|(i, _)| doc.blocks[i].message == block.message);
        let block_rect = Rect::from_min_size(origin, vec2(column.width(), laid.height));
        let hovered = ui.rect_contains_pointer(block_rect.expand2(vec2(40.0, 0.0)));
        if (hovered || (focused && caret_here)) && message.first_block == index {
            let button = match message.role {
                Role::Agent(_) => {
                    Rect::from_min_size(pos2(column.right() - 28.0, text_origin.y - 36.0), vec2(28.0, 28.0))
                }
                Role::User => {
                    let bubble = laid.frame.map_or(block_rect, |f| f.translate(origin.to_vec2()));
                    Rect::from_center_size(pos2(bubble.left() - 22.0, bubble.center().y), vec2(28.0, 28.0))
                }
            };
            if crate::components::icon_button(
                ui,
                theme,
                transcript_id().with(("copy-message", message.item.0)),
                button,
                icons::Icon::Copy,
                "Copy message",
            )
            .clicked()
            {
                ui.ctx().copy_text(message.source.clone());
                self.last_copied = Some(message.source.clone());
            }
        }

        let mut clip = painter.clip_rect();
        if let Some(frame) = laid.frame {
            let frame = frame.translate(origin.to_vec2());
            match block.kind {
                BlockKind::Code { ref lang } => {
                    painter.rect_filled(frame, theme.radius.radius_lg, c.surface_code);
                    let label = if lang.is_empty() { "code" } else { lang.as_str() };
                    painter.text(
                        pos2(frame.left() + 14.0, frame.top() + 16.0),
                        Align2::LEFT_CENTER,
                        label,
                        theme.font(&theme.text.t_caption),
                        c.text_tertiary,
                    );
                    let button =
                        Rect::from_min_size(pos2(frame.right() - 6.0 - 28.0, frame.top() + 2.0), vec2(28.0, 28.0));
                    if crate::components::icon_button(
                        ui,
                        theme,
                        transcript_id().with(("copy-code", block.id.item.0, block.id.ordinal)),
                        button,
                        icons::Icon::Copy,
                        "Copy code",
                    )
                    .clicked()
                    {
                        ui.ctx().copy_text(block.text.clone());
                        self.last_copied = Some(block.text.clone());
                    }
                    clip = clip.intersect(Rect::from_min_max(
                        pos2(frame.left() + 14.0, frame.top()),
                        pos2(frame.right() - 14.0, frame.bottom()),
                    ));
                }
                _ => {
                    painter.rect_filled(frame, theme.radius.radius_bubble, c.surface_composer);
                }
            }
        }
        let scroll = self.code_scroll.get(&block.id).copied().unwrap_or(0.0);
        let text_origin = text_origin - vec2(scroll, 0.0);
        let text_painter = painter.with_clip_rect(clip);

        if let BlockKind::ListItem { number, .. } = block.kind {
            let marker = match number {
                Some(n) => format!("{n}."),
                None => "•".to_owned(),
            };
            text_painter.text(
                pos2(text_origin.x - 8.0, text_origin.y + theme.text.t_body.line_height / 2.0),
                Align2::RIGHT_CENTER,
                marker,
                theme.font(&theme.text.t_body),
                c.text_secondary,
            );
        }

        if let Some((from, to)) = self.selection.and_then(|s| s.range_in(doc, index)) {
            paint_selection(&text_painter, &laid.galley, text_origin, from, to, theme.selection);
        }
        text_painter.galley(text_origin, laid.galley.clone(), c.text_primary);

        if focused
            && self.keyboard_caret
            && let Some(sel) = self.selection
            && let Some((i, offset)) = resolve(doc, sel.focus)
            && i == index
        {
            let caret = laid.galley.pos_from_cursor(CCursor::new(offset)).translate(text_origin.to_vec2());
            text_painter.vline(caret.left(), caret.y_range(), egui::Stroke::new(2.0, c.focus));
        }
    }

    fn paint_scrollbar(&mut self, ui: &mut Ui, theme: &Theme, rect: Rect) {
        let total = self.layout.total_height(theme);
        if total <= rect.height() + 1.0 {
            return;
        }
        let track = Rect::from_min_max(
            pos2(rect.right() - 10.0, rect.top() + 4.0),
            pos2(rect.right() - 2.0, rect.bottom() - 4.0),
        );
        let visible = rect.height() / total;
        let thumb_h = (track.height() * visible).max(24.0);
        let max_offset = total - rect.height();
        let t = (self.offset / max_offset).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(
            pos2(track.left(), track.top() + (track.height() - thumb_h) * t),
            vec2(track.width(), thumb_h),
        );
        let response = ui.interact(track, transcript_id().with("scrollbar"), Sense::drag());
        if response.dragged() {
            let dy = response.drag_delta().y;
            self.offset = (self.offset + dy / (track.height() - thumb_h).max(1.0) * max_offset).clamp(0.0, max_offset);
            self.follow = self.offset >= max_offset - 0.5;
        }
        let shown = ui.rect_contains_pointer(rect) || response.dragged();
        if shown {
            let color = if response.hovered() || response.dragged() {
                theme.color.text_tertiary
            } else {
                theme.color.surface_raised
            };
            ui.painter().rect_filled(thumb.shrink2(vec2(1.5, 0.0)), 3.0, color);
        }
    }
}

/// The conversation column: centred, at most `size-column` wide.
pub fn column_rect(rect: Rect, theme: &Theme) -> Rect {
    let width = theme.size.size_column.min(rect.width() - 2.0 * GUTTER).max(120.0);
    Rect::from_center_size(rect.center(), vec2(width, rect.height()))
}

fn paint_selection(
    painter: &egui::Painter,
    galley: &egui::Galley,
    origin: Pos2,
    from: usize,
    to: usize,
    color: egui::Color32,
) {
    let mut start = 0;
    for row in &galley.rows {
        let count = row.char_count_including_newline().0;
        let row_end = start + count;
        let a = from.max(start);
        let b = to.min(row_end);
        // Highlight the row when it overlaps, or show a sliver for a selected line break.
        if a < b || (from <= start && to > row_end) {
            let x0 = row.pos.x + row.x_offset(CharIndex(a - start));
            let mut x1 = row.pos.x + row.x_offset(CharIndex((b - start).min(row.char_count_excluding_newline().0)));
            if to > row_end.saturating_sub(1) && row.ends_with_newline || (to > row_end && b == row_end) {
                x1 += 4.0;
            }
            let r = Rect::from_min_max(pos2(x0, row.pos.y), pos2(x1.max(x0 + 2.0), row.pos.y + row.height()));
            painter.rect_filled(r.translate(origin.to_vec2()), 2.0, color);
        }
        start = row_end;
    }
    if galley.rows.is_empty() || (from == 0 && to == 0 && galley.is_empty()) {
        painter.rect_filled(Rect::from_min_size(origin, vec2(4.0, 20.0)), 2.0, color);
    }
}
