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
use egui::text_selection::accesskit_text::update_accesskit_for_text_widget;
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
        let block_ui = message_ui.new_child(
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
        update_accesskit_for_text_widget(
            &ctx,
            block_ui.unique_id(),
            None,
            role,
            TSTransform::from_translation(text_origin.to_vec2()),
            &laid.galley,
        );
        if let BlockKind::Heading(level) = block.kind {
            ctx.accesskit_node_builder(block_ui.unique_id(), |node| node.set_level(level as usize));
        }
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
