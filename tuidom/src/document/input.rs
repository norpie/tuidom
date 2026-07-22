use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::document::selection::display_to_value_offset;
use crate::error::{Result, TuidomError};
use crate::event::{InputEvent, KeyCode, KeyModifiers};
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
            state.goal_column = None;
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
            state.goal_column = None;
        }
        self.refresh_input_node(node)
    }

    pub(crate) fn apply_input_default_action(
        &self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> bool {
        let Some(node) = self.focused() else {
            return false;
        };

        match self.apply_input_default_action_to(node, code, modifiers) {
            Ok(handled) => handled,
            Err(err) => {
                tracing::error!("input default action failed: {err}");
                false
            }
        }
    }

    fn apply_input_default_action_to(
        &self,
        node: NodeId,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<bool> {
        let motion = motion_for_key(code, modifiers);

        // Page motion sizes itself on the visible box, so it needs layout and a resolved
        // style — both read here, above the node borrow. Resolving a style takes locks of
        // its own, and doing that under a `get_mut` guard is the deadlock this repo's
        // rule exists for.
        let page_rows = matches!(motion, Some(Motion::PageUp | Motion::PageDown))
            .then(|| self.input_page_rows(node))
            .flatten();

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

            // Shift is excluded deliberately: terminals report a capital as its uppercase
            // char *plus* shift, so treating any modifier as a chord would stop capital
            // letters from typing. Control and alt are what make a chord.
            let chorded = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);

            let handled = if let Some(motion) = motion {
                apply_motion(state, motion, page_rows)
            } else {
                match code {
                    // A control chord arrives as its plain letter — ctrl+a is `Char('a')`
                    // with control held, not a control character — so without this guard
                    // every ctrl and alt chord would type its letter into the input.
                    KeyCode::Char(ch) if !ch.is_control() && !chorded => {
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
                    KeyCode::Enter => {
                        if state.multiline {
                            replace_selection_or_insert(state, "\n");
                            value_changed = true;
                        }
                        true
                    }
                    _ => false,
                }
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

    /// How many content rows one page motion covers in an input, per the last layout.
    ///
    /// One row of overlap is kept, so a page leaves a shared line to read against rather
    /// than replacing the whole view. Returns `None` before the input has been laid out,
    /// which makes page motion a no-op rather than a guess.
    fn input_page_rows(&self, node: NodeId) -> Option<usize> {
        let layout = self.get_node(node).and_then(|view| view.layout)?;
        let content = layout.rect.content_rect(&self.resolved_style(node).ok()?);
        Some(usize::from(content.height.saturating_sub(1)).max(1))
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

/// A cursor movement an input binding asks for, independent of the key that asked.
///
/// Separating the request from the key is what keeps the binding table from having to
/// spell out every motion twice once shift-extension arrives — the motion is the same
/// either way, only what happens to the selection differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    /// One grapheme back.
    Left,
    /// One grapheme forward.
    Right,
    /// Start of the current line.
    LineStart,
    /// End of the current line.
    LineEnd,
    /// One display row up, holding the goal column. Multiline only.
    LineUp,
    /// One display row down, holding the goal column. Multiline only.
    LineDown,
    /// A visible page up, holding the goal column. Multiline only.
    PageUp,
    /// A visible page down, holding the goal column. Multiline only.
    PageDown,
    /// Start of the whole value.
    ValueStart,
    /// End of the whole value.
    ValueEnd,
    /// Start of the previous word.
    WordLeft,
    /// Start of the next word.
    WordRight,
}

impl Motion {
    /// Whether this motion moves between display rows, and so maintains the goal column.
    fn is_vertical(self) -> bool {
        matches!(
            self,
            Motion::LineUp | Motion::LineDown | Motion::PageUp | Motion::PageDown
        )
    }
}

/// The motion a key press asks an input for, if any.
///
/// Shift is masked off rather than matched: a terminal reports a capital as its uppercase
/// char plus shift, and until shift means "extend the selection" it must not change which
/// motion a key names. Control chords are matched exactly, so ctrl+up stays unbound and
/// reaches focus navigation instead of silently acting like a plain up.
fn motion_for_key(code: KeyCode, modifiers: KeyModifiers) -> Option<Motion> {
    let control = modifiers.contains(KeyModifiers::CONTROL);
    if modifiers.contains(KeyModifiers::ALT) {
        return None;
    }

    match (code, control) {
        (KeyCode::Left, false) => Some(Motion::Left),
        (KeyCode::Right, false) => Some(Motion::Right),
        (KeyCode::Home, false) => Some(Motion::LineStart),
        (KeyCode::End, false) => Some(Motion::LineEnd),
        (KeyCode::Up, false) => Some(Motion::LineUp),
        (KeyCode::Down, false) => Some(Motion::LineDown),
        (KeyCode::PageUp, false) => Some(Motion::PageUp),
        (KeyCode::PageDown, false) => Some(Motion::PageDown),
        (KeyCode::Left, true) => Some(Motion::WordLeft),
        (KeyCode::Right, true) => Some(Motion::WordRight),
        (KeyCode::Home, true) => Some(Motion::ValueStart),
        (KeyCode::End, true) => Some(Motion::ValueEnd),
        _ => None,
    }
}

/// Move an input's cursor, reporting whether the motion applied.
///
/// An unapplied motion is not handled, so the key falls through to the document's own
/// default actions — which is how a single-line input lets up and down reach focus
/// navigation instead of eating them.
fn apply_motion(state: &mut InputState, motion: Motion, page_rows: Option<usize>) -> bool {
    // A collapsed motion out of a selection lands on the selection's edge rather than
    // moving from the cursor, matching how every editor leaves a selection.
    if !motion.is_vertical()
        && let Some(selection) = state.selection.clone()
    {
        let edge = match motion {
            Motion::Left => Some(selection.start),
            Motion::Right => Some(selection.end),
            _ => None,
        };
        if let Some(edge) = edge {
            state.selection = None;
            state.goal_column = None;
            state.cursor = edge;
            return true;
        }
    }

    let Some(target) = motion_target(state, motion, page_rows) else {
        return false;
    };

    state.selection = None;
    state.cursor = target;
    if !motion.is_vertical() {
        state.goal_column = None;
    }
    true
}

/// The byte offset a motion lands on, or `None` when it does not apply.
///
/// Vertical motions update the goal column as a side effect, since the column a run of
/// them holds is established by the first one and has to outlive it.
fn motion_target(
    state: &mut InputState,
    motion: Motion,
    page_rows: Option<usize>,
) -> Option<usize> {
    // A motion that has run out of room lands where it is rather than declining, so the
    // key stays handled and an arrow at the end of the value cannot fall through to focus
    // navigation. Declining is reserved for motions that do not apply at all.
    //
    // An unmeasured input pages by a single row: the same degradation an editor makes
    // before it knows its own height, and better than leaving the key unhandled.
    let rows = page_rows
        .and_then(|rows| i64::try_from(rows).ok())
        .unwrap_or(1);

    match motion {
        Motion::Left => Some(previous_grapheme_boundary(&state.content, state.cursor).unwrap_or(0)),
        Motion::Right => Some(
            next_grapheme_boundary(&state.content, state.cursor).unwrap_or(state.content.len()),
        ),
        Motion::LineStart => Some(line_start(&state.content, state.cursor)),
        Motion::LineEnd => Some(line_end(&state.content, state.cursor)),
        Motion::ValueStart => Some(0),
        Motion::ValueEnd => Some(state.content.len()),
        Motion::WordLeft => Some(previous_word_boundary(&state.content, state.cursor)),
        Motion::WordRight => Some(next_word_boundary(&state.content, state.cursor)),
        Motion::LineUp => vertical_target(state, -1),
        Motion::LineDown => vertical_target(state, 1),
        Motion::PageUp => vertical_target(state, -rows),
        Motion::PageDown => vertical_target(state, rows),
    }
}

/// The offset `rows` display rows away, holding the goal column.
///
/// Single-line inputs have one row and so no vertical motion at all; they return `None`
/// rather than clamping to the same offset, so the key stays unhandled.
fn vertical_target(state: &mut InputState, rows: i64) -> Option<usize> {
    if !state.multiline {
        return None;
    }

    let display = crate::node::input_display_content(&state.content, true, state.mask);
    let lines: Vec<&str> = display.split('\n').collect();
    let position = input_cursor_position(state);

    let target_line = i64::from(position.y).saturating_add(rows);
    let target_line = target_line.clamp(0, (lines.len() as i64).saturating_sub(1)) as usize;
    if target_line == usize::from(position.y) {
        // Already on the edge row. Handled, but unmoved: a key does not chain to the
        // container behind it the way a wheel does.
        return Some(state.cursor);
    }

    let column = state.goal_column.unwrap_or(position.x);
    state.goal_column = Some(column);

    let line = lines.get(target_line)?;
    let line_start: usize = lines.get(..target_line)?.iter().map(|l| l.len() + 1).sum();
    Some(display_to_value_offset(
        &state.content,
        &display,
        line_start + column_offset_in_line(line, column),
    ))
}

/// The byte offset into a display line at `column` cells, clamped to the line's end.
fn column_offset_in_line(line: &str, column: u16) -> usize {
    let mut col = 0u16;
    for (offset, grapheme) in line.grapheme_indices(true) {
        let width = UnicodeWidthStr::width(grapheme).min(2) as u16;
        if width == 0 {
            continue;
        }
        // A wide glyph straddling the goal column counts as containing it, so a run of
        // vertical motion cannot land between a glyph's two cells.
        if column < col + width {
            return offset;
        }
        col += width;
    }
    line.len()
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
    // Every edit and every programmatic write funnels through here, and all of them end a
    // run of vertical motion — the column the run was holding no longer describes content
    // that still exists.
    state.goal_column = None;
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

/// The start of the word before `cursor`, or 0.
///
/// Whitespace-only runs are skipped rather than treated as words, so a run of spaces
/// between two words costs one press rather than two.
fn previous_word_boundary(content: &str, cursor: usize) -> usize {
    let cursor = clamp_to_grapheme_boundary(content, cursor);
    content
        .split_word_bound_indices()
        .rfind(|(index, word)| *index < cursor && is_word_like(word))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// The start of the word after `cursor`, or the end of the content.
fn next_word_boundary(content: &str, cursor: usize) -> usize {
    let cursor = clamp_to_grapheme_boundary(content, cursor);
    content
        .split_word_bound_indices()
        .find(|(index, word)| *index > cursor && is_word_like(word))
        .map(|(index, _)| index)
        .unwrap_or(content.len())
}

fn is_word_like(word: &str) -> bool {
    !word.chars().all(char::is_whitespace)
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
