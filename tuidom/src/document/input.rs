use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::event::{InputEvent, KeyCode};
use crate::id::NodeId;
use crate::node::{InputState, NodeKind};

impl Document {
    /// Return an input node's stored value.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn input_value(&self, node: NodeId) -> Result<String> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state(&data.kind, node)?;
        Ok(state.content.clone())
    }

    /// Replace an input node's stored value.
    ///
    /// Cursor and selection offsets are clamped to grapheme boundaries in the
    /// new value.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn set_input_value(&self, node: NodeId, value: impl Into<String>) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.content = value.into();
            normalize_input_state(state);
        }
        self.refresh_input_node(node)
    }

    /// Return an input node's cursor byte offset.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn input_cursor(&self, node: NodeId) -> Result<usize> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state(&data.kind, node)?;
        Ok(state.cursor)
    }

    /// Set an input node's cursor byte offset.
    ///
    /// The stored cursor is clamped to the nearest previous grapheme boundary
    /// within the input value. Setting the cursor clears any active selection.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn set_input_cursor(&self, node: NodeId, cursor: usize) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.cursor = clamp_to_grapheme_boundary(&state.content, cursor);
            state.selection = None;
        }
        self.refresh_input_node(node)
    }

    /// Return an input node's selected byte range, if any.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn input_selection(&self, node: NodeId) -> Result<Option<Range<usize>>> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state(&data.kind, node)?;
        Ok(state.selection.clone())
    }

    /// Set an input node's selected byte range.
    ///
    /// Range endpoints are clamped to grapheme boundaries and reordered if the
    /// range is reversed. Collapsed ranges clear the selection.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn set_input_selection(&self, node: NodeId, selection: Range<usize>) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.selection = normalize_selection(&state.content, selection);
        }
        self.refresh_input_node(node)
    }

    /// Clear an input node's selection.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn clear_input_selection(&self, node: NodeId) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.selection = None;
        }
        self.refresh_input_node(node)
    }

    /// Return whether an input accepts newlines.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn input_multiline(&self, node: NodeId) -> Result<bool> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state(&data.kind, node)?;
        Ok(state.multiline)
    }

    /// Set whether an input accepts newlines.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn set_input_multiline(&self, node: NodeId, multiline: bool) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.multiline = multiline;
        }
        self.refresh_input_node(node)
    }

    /// Return an input's display mask character.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn input_mask(&self, node: NodeId) -> Result<Option<char>> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state(&data.kind, node)?;
        Ok(state.mask)
    }

    /// Set an input's display mask character.
    ///
    /// This does not change the stored input value.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an input node.
    pub fn set_input_mask(&self, node: NodeId, mask: Option<char>) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            state.mask = mask;
        }
        self.refresh_input_node(node)
    }

    /// Drive an input's selection from a mouse drag between two value offsets.
    ///
    /// The endpoints are terminal-inclusive like document selection: the later one
    /// extends past the glyph under it, unless that glyph is a line break. The cursor
    /// follows the drag's focus end.
    pub(crate) fn drive_input_drag(&self, node: NodeId, anchor: usize, focus: usize) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let state = input_state_mut(&mut data.kind, node)?;
            let low = anchor.min(focus).min(state.content.len());
            let high = anchor.max(focus).min(state.content.len());
            let high = match state
                .content
                .get(high..)
                .and_then(|s| s.graphemes(true).next())
            {
                Some(grapheme) if grapheme != "\n" => high + grapheme.len(),
                _ => high,
            };
            state.selection = normalize_selection(&state.content, low..high);
            state.cursor = clamp_to_grapheme_boundary(&state.content, focus);
        }
        self.refresh_input_node(node)
    }

    pub(crate) fn apply_input_default_action(&self, code: KeyCode) -> bool {
        let Some(node) = self.focused() else {
            return false;
        };

        match self.apply_input_default_action_to(node, code) {
            Ok(handled) => handled,
            Err(err) => {
                log::error!("input default action failed: {err}");
                false
            }
        }
    }

    fn apply_input_default_action_to(&self, node: NodeId, code: KeyCode) -> Result<bool> {
        // An input measures on its content, so a value change is exactly what needs
        // relayout — and exactly what `on_input` reports. The two share one flag because
        // they are the same condition, not because they happen to coincide today.
        let mut value_changed = false;
        let (handled, value) = {
            let Some(mut data) = self.inner.nodes.get_mut(&node) else {
                return Ok(false);
            };
            let Ok(state) = input_state_mut(&mut data.kind, node) else {
                return Ok(false);
            };

            let handled = match code {
                KeyCode::Char(ch) if !ch.is_control() => {
                    replace_selection_or_insert(state, &ch.to_string());
                    value_changed = true;
                    true
                }
                KeyCode::Backspace => {
                    value_changed = delete_selection_or_previous_grapheme(state);
                    true
                }
                KeyCode::Delete => {
                    value_changed = delete_selection_or_next_grapheme(state);
                    true
                }
                KeyCode::Left => {
                    move_cursor_left(state);
                    true
                }
                KeyCode::Right => {
                    move_cursor_right(state);
                    true
                }
                KeyCode::Home => {
                    move_cursor_to_line_start(state);
                    true
                }
                KeyCode::End => {
                    move_cursor_to_line_end(state);
                    true
                }
                KeyCode::Up | KeyCode::Down => true,
                KeyCode::Enter => {
                    if state.multiline {
                        replace_selection_or_insert(state, "\n");
                        value_changed = true;
                    }
                    true
                }
                _ => false,
            };

            // Cloned inside the borrow, dispatched outside it: a handler is downstream
            // code and may touch this very node, which would deadlock against the guard.
            (handled, value_changed.then(|| state.content.clone()))
        };

        if handled {
            self.update_input_scroll(node)?;
            if value_changed {
                self.register_layout_node(node)?;
            }
            self.inner.notify.notify_one();
        }

        if let Some(value) = value {
            self.dispatch_input_to(node, &mut InputEvent::new(value));
        }

        Ok(handled)
    }

    fn refresh_input_node(&self, node: NodeId) -> Result<()> {
        self.update_input_scroll(node)?;
        self.register_layout_node(node)?;
        self.inner.notify.notify_one();
        Ok(())
    }

    fn update_input_scroll(&self, node: NodeId) -> Result<()> {
        if self.focused() != Some(node) {
            return Ok(());
        }
        let Some(layout) = self.get_node(node).and_then(|view| view.layout) else {
            return Ok(());
        };
        // Scroll against the rect the glyphs are actually written into, so a padded input
        // does not believe it has more room than it paints.
        let content = layout.rect.content_rect(&self.resolved_style(node)?);
        if content.width == 0 || content.height == 0 {
            return Ok(());
        }

        let mut data = self
            .inner
            .nodes
            .get_mut(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let state = input_state_mut(&mut data.kind, node)?;
        keep_cursor_visible(state, content.width, content.height);
        Ok(())
    }
}

fn input_state(kind: &NodeKind, node: NodeId) -> Result<&InputState> {
    match kind {
        NodeKind::Input { state } => Ok(state),
        _ => Err(TuidomError::NodeNotInput { id: node }),
    }
}

fn input_state_mut(kind: &mut NodeKind, node: NodeId) -> Result<&mut InputState> {
    match kind {
        NodeKind::Input { state } => Ok(state),
        _ => Err(TuidomError::NodeNotInput { id: node }),
    }
}

fn replace_selection_or_insert(state: &mut InputState, text: &str) {
    let range = state.selection.take().unwrap_or(state.cursor..state.cursor);
    state.content.replace_range(range.clone(), text);
    state.cursor = range.start + text.len();
    normalize_input_state(state);
}

fn delete_selection_or_previous_grapheme(state: &mut InputState) -> bool {
    if let Some(selection) = state.selection.take() {
        state.content.replace_range(selection.clone(), "");
        state.cursor = selection.start;
        normalize_input_state(state);
        return true;
    }

    let Some(previous) = previous_grapheme_boundary(&state.content, state.cursor) else {
        return false;
    };
    state.content.replace_range(previous..state.cursor, "");
    state.cursor = previous;
    normalize_input_state(state);
    true
}

fn delete_selection_or_next_grapheme(state: &mut InputState) -> bool {
    if let Some(selection) = state.selection.take() {
        state.content.replace_range(selection.clone(), "");
        state.cursor = selection.start;
        normalize_input_state(state);
        return true;
    }

    let Some(next) = next_grapheme_boundary(&state.content, state.cursor) else {
        return false;
    };
    state.content.replace_range(state.cursor..next, "");
    normalize_input_state(state);
    true
}

fn move_cursor_left(state: &mut InputState) {
    if let Some(selection) = state.selection.take() {
        state.cursor = selection.start;
        return;
    }
    if let Some(previous) = previous_grapheme_boundary(&state.content, state.cursor) {
        state.cursor = previous;
    }
}

fn move_cursor_right(state: &mut InputState) {
    if let Some(selection) = state.selection.take() {
        state.cursor = selection.end;
        return;
    }
    if let Some(next) = next_grapheme_boundary(&state.content, state.cursor) {
        state.cursor = next;
    }
}

fn move_cursor_to_line_start(state: &mut InputState) {
    state.selection = None;
    state.cursor = line_start(&state.content, state.cursor);
}

fn move_cursor_to_line_end(state: &mut InputState) {
    state.selection = None;
    state.cursor = line_end(&state.content, state.cursor);
}

fn keep_cursor_visible(state: &mut InputState, width: u16, height: u16) {
    let position = input_cursor_position(state);

    if position.x < state.scroll_x {
        state.scroll_x = position.x;
    } else {
        let right = state.scroll_x.saturating_add(width);
        let cursor_right = position.x.saturating_add(u16::from(position.width));
        if cursor_right > right {
            state.scroll_x = cursor_right.saturating_sub(width);
        }
    }

    if state.multiline {
        if position.y < state.scroll_y {
            state.scroll_y = position.y;
        } else {
            let bottom = state.scroll_y.saturating_add(height);
            let cursor_bottom = position.y.saturating_add(1);
            if cursor_bottom > bottom {
                state.scroll_y = cursor_bottom.saturating_sub(height);
            }
        }
    } else {
        state.scroll_y = 0;
    }
}

#[derive(Debug, Clone, Copy)]
struct InputCursorPosition {
    x: u16,
    y: u16,
    width: u8,
}

fn input_cursor_position(state: &InputState) -> InputCursorPosition {
    let cursor = clamp_to_grapheme_boundary(&state.content, state.cursor);
    let prefix =
        crate::node::input_display_content(&state.content[..cursor], state.multiline, state.mask);
    let y = prefix.matches('\n').count().min(u16::MAX as usize) as u16;
    let x = UnicodeWidthStr::width(prefix.rsplit('\n').next().unwrap_or("")).min(u16::MAX as usize)
        as u16;
    let width = cursor_grapheme_width(&state.content[cursor..], state.multiline, state.mask);
    InputCursorPosition { x, y, width }
}

fn cursor_grapheme_width(suffix: &str, multiline: bool, mask: Option<char>) -> u8 {
    let Some(grapheme) = suffix.graphemes(true).next() else {
        return 1;
    };
    if multiline && grapheme == "\n" {
        return 1;
    }

    let display = if let Some(mask) = mask {
        mask.to_string()
    } else if !multiline && grapheme == "\n" {
        " ".to_owned()
    } else {
        grapheme.to_owned()
    };
    UnicodeWidthStr::width(display.as_str()).clamp(1, 2) as u8
}

fn normalize_input_state(state: &mut InputState) {
    state.cursor = clamp_to_grapheme_boundary(&state.content, state.cursor);
    if let Some(selection) = state.selection.take() {
        state.selection = normalize_selection(&state.content, selection);
    }
}

fn normalize_selection(content: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let a = clamp_to_grapheme_boundary(content, selection.start);
    let b = clamp_to_grapheme_boundary(content, selection.end);
    let start = a.min(b);
    let end = a.max(b);
    (start != end).then_some(start..end)
}

pub(crate) fn clamp_to_grapheme_boundary(content: &str, offset: usize) -> usize {
    if offset >= content.len() {
        return content.len();
    }

    content
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index <= offset)
        .last()
        .unwrap_or(0)
}

fn previous_grapheme_boundary(content: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    let cursor = clamp_to_grapheme_boundary(content, cursor);
    content
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index < cursor)
        .last()
        .or(Some(0))
}

fn next_grapheme_boundary(content: &str, cursor: usize) -> Option<usize> {
    let cursor = clamp_to_grapheme_boundary(content, cursor);
    if cursor >= content.len() {
        return None;
    }

    content
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index > cursor)
        .or(Some(content.len()))
}

fn line_start(content: &str, cursor: usize) -> usize {
    let cursor = clamp_to_grapheme_boundary(content, cursor);
    content[..cursor]
        .rfind('\n')
        .map(|index| index + '\n'.len_utf8())
        .unwrap_or(0)
}

fn line_end(content: &str, cursor: usize) -> usize {
    let cursor = clamp_to_grapheme_boundary(content, cursor);
    content[cursor..]
        .find('\n')
        .map(|index| cursor + index)
        .unwrap_or(content.len())
}
