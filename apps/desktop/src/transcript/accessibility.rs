//! The transcript's AccessKit tree, fed by the same layout that paints it.
//!
//! The widget is one Document node that owns the text selection. Each
//! message in view is an article named by who wrote it, and each block is a
//! paragraph, heading, list item or code node with text runs, so screen
//! readers get message boundaries and readable, selectable text. Blocks far
//! outside the viewport are not exposed; a selection end there is reported
//! at the edge of the exposed text.

use std::collections::HashMap;

use egui::accesskit::{self, Role};
use egui::emath::TSTransform;
use egui::text::CCursor;
use egui::{Id, Pos2, Rect, Ui, UiBuilder, vec2};

use super::document::{BlockId, BlockKind, Document, Role as Author};
use super::layout::Layout;
use super::selection::{Selection, resolve};

/// AccessKit splits rows into text runs of at most this many characters.
/// Mirrors egui's text-run scheme so positions point at its run nodes.
const MAX_CHARS_PER_TEXT_RUN: usize = 255;

pub fn message_node_id(doc_id: Id, index: usize) -> Id {
    doc_id.with(("message", index))
}

pub fn block_node_id(doc_id: Id, block: BlockId) -> Id {
    doc_id.with(("block", block.item.0, block.ordinal))
}

pub fn publish(
    ui: &mut Ui,
    doc_id: Id,
    doc: &Document,
    layout: &Layout,
    positions: &[(usize, Pos2)],
    selection: Option<Selection>,
    code_scroll: &HashMap<BlockId, f32>,
) {
    let ctx = ui.ctx().clone();
    let total = doc.messages.len();
    ctx.accesskit_node_builder(doc_id, |node| {
        node.set_role(Role::Document);
        node.set_label("Conversation");
        node.set_description(format!("{total} messages"));
        node.set_read_only();
    });

    let mut current_message: Option<(usize, Ui)> = None;
    let mut exposed: Vec<usize> = Vec::new();
    for &(index, origin) in positions {
        let block = &doc.blocks[index];
        let Some(laid) = layout.cached(block.id) else { continue };
        if current_message.as_ref().map(|(m, _)| *m) != Some(block.message) {
            let message = &doc.messages[block.message];
            let id = message_node_id(doc_id, block.message);
            let message_ui = ui.new_child(UiBuilder::new().id(id).accessibility_parent(doc_id).max_rect(ui.max_rect()));
            let author = match message.role {
                Author::User => "You".to_owned(),
                Author::Agent(provider) => crate::components::provider_name(provider).to_owned(),
            };
            let number = block.message + 1;
            ctx.accesskit_node_builder(id, |node| {
                node.set_role(Role::Article);
                node.set_label(format!("{author}, message {number} of {total}"));
                if !message.completed {
                    node.set_busy();
                }
            });
            current_message = Some((block.message, message_ui));
        }
        let Some((_, message_ui)) = current_message.as_mut() else { continue };

        let node_id = block_node_id(doc_id, block.id);
        let scroll = code_scroll.get(&block.id).copied().unwrap_or(0.0);
        let text_origin = origin + laid.text_pos - vec2(scroll, 0.0);
        let mut block_ui = message_ui.new_child(
            UiBuilder::new()
                .id(node_id)
                .accessibility_parent(message_node_id(doc_id, block.message))
                .max_rect(Rect::from_min_size(origin, laid.galley.size())),
        );
        let role = match block.kind {
            BlockKind::Paragraph => Role::Paragraph,
            BlockKind::Heading(_) => Role::Heading,
            BlockKind::ListItem { .. } => Role::ListItem,
            BlockKind::Code { .. } => Role::Code,
        };
        ctx.accesskit_node_builder(node_id, |node| {
            node.set_role(role);
            if let BlockKind::Heading(level) = block.kind {
                node.set_level(level as usize);
            }
        });
        // End each block with the same break that copy uses, so a screen
        // reader never runs the last word of one block into the next.
        let separator = match doc.blocks.get(index + 1) {
            Some(next)
                if next.message == block.message
                    && matches!(block.kind, BlockKind::ListItem { .. })
                    && matches!(next.kind, BlockKind::ListItem { .. }) =>
            {
                "\n"
            }
            Some(_) => "\n\n",
            None => "",
        };
        text_runs(
            &mut block_ui,
            node_id,
            &laid.galley,
            TSTransform::from_translation(text_origin.to_vec2()),
            separator,
        );
        exposed.push(index);
    }

    let (Some(sel), Some(&first), Some(&last)) = (selection, exposed.first(), exposed.last()) else {
        return;
    };
    let clamp = |pos| {
        let (index, offset) = resolve(doc, pos)?;
        Some(if index < first {
            (first, 0)
        } else if index > last {
            (last, doc.blocks[last].chars)
        } else {
            (index, offset)
        })
    };
    let (Some(anchor), Some(focus)) = (clamp(sel.anchor), clamp(sel.focus)) else { return };
    let (Some(anchor), Some(focus)) =
        (text_position(doc_id, doc, layout, anchor), text_position(doc_id, doc, layout, focus))
    else {
        return;
    };
    ctx.accesskit_node_builder(doc_id, |node| {
        node.set_text_selection(accesskit::TextSelection { anchor, focus });
    });
}

fn text_position(
    doc_id: Id,
    doc: &Document,
    layout: &Layout,
    (index, offset): (usize, usize),
) -> Option<accesskit::TextPosition> {
    let block = &doc.blocks[index];
    let laid = layout.cached(block.id)?;
    let cursor = laid.galley.layout_from_cursor(CCursor::new(offset));
    let column = cursor.column.0;
    let chunk = if column > 0 && column.is_multiple_of(MAX_CHARS_PER_TEXT_RUN) {
        column / MAX_CHARS_PER_TEXT_RUN - 1
    } else {
        column / MAX_CHARS_PER_TEXT_RUN
    };
    Some(accesskit::TextPosition {
        node: block_node_id(doc_id, block.id).with(cursor.row).with(chunk).accesskit_id(),
        character_index: column - chunk * MAX_CHARS_PER_TEXT_RUN,
    })
}

/// One text run per laid-out row (split at 255 characters), as children of
/// the block node. The block's separator is appended to its last run.
fn text_runs(block_ui: &mut Ui, node_id: Id, galley: &egui::Galley, transform: TSTransform, separator: &str) {
    let ctx = block_ui.ctx().clone();
    let rows = galley.rows.len();
    for (row_index, row) in galley.rows.iter().enumerate() {
        let mut value = String::new();
        let mut lengths = Vec::<u8>::new();
        let mut positions = Vec::<f32>::new();
        let mut widths = Vec::<f32>::new();
        let mut word_starts = Vec::<usize>::new();
        let mut was_word_end = row_index > 0 && !galley.rows[row_index - 1].ends_with_newline;
        for glyph in &row.glyphs {
            let word_char = glyph.chr.is_alphanumeric();
            if word_char && was_word_end {
                word_starts.push(lengths.len());
            }
            was_word_end = !word_char;
            let before = value.len();
            value.push(glyph.chr);
            lengths.push((value.len() - before) as u8);
            positions.push(glyph.pos.x - row.pos.x);
            widths.push(glyph.advance_width);
        }
        let mut tail = String::new();
        if row.ends_with_newline {
            tail.push('\n');
        }
        if row_index + 1 == rows {
            tail.push_str(separator);
        }
        for c in tail.chars() {
            value.push(c);
            lengths.push(1);
            positions.push(row.size.x);
            widths.push(0.0);
        }

        let chunks = lengths.len().div_ceil(MAX_CHARS_PER_TEXT_RUN).max(1);
        let mut byte = 0;
        for chunk in 0..chunks {
            let from = chunk * MAX_CHARS_PER_TEXT_RUN;
            let to = (from + MAX_CHARS_PER_TEXT_RUN).min(lengths.len());
            let bytes: usize = lengths[from..to].iter().map(|&l| l as usize).sum();
            let text = value[byte..byte + bytes].to_owned();
            byte += bytes;
            let run_id = node_id.with(row_index).with(chunk);
            // A child Ui registers the run's parent; egui has no public call for it.
            let _ = block_ui.new_child(UiBuilder::new().id(run_id).accessibility_parent(node_id));
            let x0 = row.pos.x + positions.get(from).copied().unwrap_or(0.0);
            let x1 = row.pos.x
                + positions.get(to.saturating_sub(1)).copied().unwrap_or(0.0)
                + widths.get(to.saturating_sub(1)).copied().unwrap_or(0.0);
            let rect = transform
                * Rect::from_min_max(egui::pos2(x0, row.pos.y), egui::pos2(x1.max(x0), row.pos.y + row.height()));
            let offset = positions.get(from).copied().unwrap_or(0.0);
            let starts: Vec<u8> =
                word_starts.iter().filter(|&&w| w >= from && w < to).map(|&w| (w - from) as u8).collect();
            ctx.accesskit_node_builder(run_id, |node| {
                node.set_role(Role::TextRun);
                node.set_text_direction(accesskit::TextDirection::LeftToRight);
                node.set_bounds(accesskit::Rect {
                    x0: rect.min.x.into(),
                    y0: rect.min.y.into(),
                    x1: rect.max.x.into(),
                    y1: rect.max.y.into(),
                });
                node.set_value(text);
                node.set_character_lengths(lengths[from..to].to_vec());
                node.set_character_positions(positions[from..to].iter().map(|p| p - offset).collect::<Vec<_>>());
                node.set_character_widths(widths[from..to].to_vec());
                node.set_word_starts(starts);
                if chunk > 0 {
                    node.set_previous_on_line(node_id.with(row_index).with(chunk - 1).accesskit_id());
                }
                if chunk + 1 < chunks {
                    node.set_next_on_line(node_id.with(row_index).with(chunk + 1).accesskit_id());
                }
            });
        }
    }
}
