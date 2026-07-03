use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::document::Document;
use crate::error::{Result, TuidomError};
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

    fn refresh_input_node(&self, node: NodeId) -> Result<()> {
        self.register_layout_node(node)?;
        self.inner.notify.notify_one();
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

fn clamp_to_grapheme_boundary(content: &str, offset: usize) -> usize {
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
