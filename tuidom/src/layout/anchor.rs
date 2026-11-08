//! Placing [`Position::Anchored`] nodes once taffy has placed everything else.
//!
//! Taffy lays an anchored node out at its parent's origin. This pass moves it, with its
//! subtree, to where its anchor is drawn. The rect it writes has the node's own ancestors'
//! scrolling added back, so paint's usual translation lands the node beside the anchor, and
//! every reader of the snapshot sees it there without knowing about anchoring.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::document::Document;
use crate::error::Result;
use crate::id::{NodeId, NodeMap};
use crate::node::{LayoutRect, NodeLayout, ScrollOffset};
use crate::paint_order::DrawContext;
use crate::style::resolution::ResolvedStyle;
use crate::style::{AnchorAxis, AnchorEdge, Position};

struct Anchored {
    node: NodeId,
    x: AnchorAxis,
    y: AnchorAxis,
    /// Root first, without the node.
    ancestors: Vec<NodeId>,
    /// Root first, ending with the anchor. `None` when the anchor is not in the visible tree.
    anchor_chain: Option<Vec<NodeId>>,
}

/// Every anchored node in the visible tree, with the chains and styles its placement reads.
///
/// Collected with the visible tree, before the layout lock is taken, so placement under that
/// lock resolves no styles.
pub(super) struct Anchoring {
    nodes: Vec<Anchored>,
    styles: NodeMap<Arc<ResolvedStyle>>,
}

impl Anchoring {
    pub(super) fn collect(
        doc: &Document,
        anchored: &[NodeId],
        visible: &HashSet<NodeId>,
    ) -> Result<Self> {
        let mut styles = NodeMap::default();
        let mut nodes = Vec::with_capacity(anchored.len());
        for &node in anchored {
            let Position::Anchored { anchor, x, y } = doc.resolved_style_unlocked(node)?.position
            else {
                continue;
            };
            let ancestors = ancestors_of(doc, node);
            let anchor_chain = visible.contains(&anchor).then(|| {
                let mut chain = ancestors_of(doc, anchor);
                chain.push(anchor);
                chain
            });
            for &id in ancestors.iter().chain(anchor_chain.iter().flatten()) {
                if let Entry::Vacant(entry) = styles.entry(id) {
                    entry.insert(doc.resolved_style_unlocked(id)?);
                }
            }
            nodes.push(Anchored {
                node,
                x,
                y,
                ancestors,
                anchor_chain,
            });
        }
        Ok(Self { nodes, styles })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Move every anchored node in `layouts` to its anchor, and remove the subtree of each one
    /// that cannot be placed.
    pub(super) fn place(
        &self,
        layouts: &mut NodeMap<NodeLayout>,
        children: &HashMap<NodeId, Vec<NodeId>>,
        offsets: &NodeMap<ScrollOffset>,
        screen: (u16, u16),
    ) {
        let mut placement = Placement {
            anchoring: self,
            index: self
                .nodes
                .iter()
                .enumerate()
                .map(|(i, anchored)| (anchored.node, i))
                .collect(),
            states: vec![State::Pending; self.nodes.len()],
            layouts,
            children,
            offsets,
            screen,
        };
        for i in 0..self.nodes.len() {
            placement.resolve(i);
        }
    }
}

fn ancestors_of(doc: &Document, node: NodeId) -> Vec<NodeId> {
    let mut ancestors = Vec::new();
    let mut current = node;
    while let Some(parent) = doc.inner.nodes.get(&current).and_then(|data| data.parent) {
        ancestors.push(parent);
        current = parent;
    }
    ancestors.reverse();
    ancestors
}

#[derive(Clone, Copy)]
enum State {
    Pending,
    Placing,
    Done,
}

struct Placement<'a> {
    anchoring: &'a Anchoring,
    index: NodeMap<usize>,
    states: Vec<State>,
    layouts: &'a mut NodeMap<NodeLayout>,
    children: &'a HashMap<NodeId, Vec<NodeId>>,
    offsets: &'a NodeMap<ScrollOffset>,
    screen: (u16, u16),
}

impl Placement<'_> {
    /// Place one anchored node, after every anchored node that moves it or its anchor.
    fn resolve(&mut self, i: usize) {
        if !matches!(self.states[i], State::Pending) {
            return;
        }
        self.states[i] = State::Placing;
        let anchoring = self.anchoring;
        let anchored = &anchoring.nodes[i];
        if !(self.place_dependencies(anchored) && self.place(anchored)) {
            self.remove_subtree(anchored.node);
        }
        self.states[i] = State::Done;
    }

    /// False when a dependency is still being placed, which is a cycle.
    fn place_dependencies(&mut self, anchored: &Anchored) -> bool {
        let chains = anchored
            .ancestors
            .iter()
            .chain(anchored.anchor_chain.iter().flatten());
        for id in chains {
            let Some(&j) = self.index.get(id) else {
                continue;
            };
            match self.states[j] {
                State::Placing => return false,
                State::Pending => self.resolve(j),
                State::Done => {}
            }
        }
        true
    }

    fn place(&mut self, anchored: &Anchored) -> bool {
        let Some((&anchor, anchor_ancestors)) = anchored
            .anchor_chain
            .as_ref()
            .and_then(|chain| chain.split_last())
        else {
            return false;
        };
        let Some(anchor_context) = self.context_inside(anchor_ancestors) else {
            return false;
        };
        let Some(anchor_rect) = self.layouts.get(&anchor).map(|layout| layout.rect) else {
            return false;
        };
        let target = anchor_context.place(anchor_rect);
        if anchor_context.clip.visible_part(target).is_none() {
            return false;
        }
        let Some(own_context) = self.context_inside(&anchored.ancestors) else {
            return false;
        };
        let Some(current) = self.layouts.get(&anchored.node).map(|layout| layout.rect) else {
            return false;
        };

        let x = i64::from(target.x)
            + along(anchored.x, target.width, current.width)
            + i64::from(own_context.translation.0);
        let y = i64::from(target.y)
            + along(anchored.y, target.height, current.height)
            + i64::from(own_context.translation.1);
        self.shift_subtree(
            anchored.node,
            x - i64::from(current.x),
            y - i64::from(current.y),
        );
        true
    }

    /// The context a child of the chain's last node draws in. The terminal bounds it, so the
    /// clip says whether anything reaches the screen.
    fn context_inside(&self, chain: &[NodeId]) -> Option<DrawContext> {
        let (width, height) = self.screen;
        let mut context = DrawContext::screen(width, height);
        for id in chain {
            let rect = self.layouts.get(id)?.rect;
            let resolved = self.anchoring.styles.get(id)?;
            let offset = || self.offsets.get(id).copied().unwrap_or_default();
            context = context.children_of(context.place(rect), resolved, offset);
        }
        Some(context)
    }

    fn shift_subtree(&mut self, node: NodeId, dx: i64, dy: i64) {
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            if let Some(layout) = self.layouts.get_mut(&id) {
                layout.rect = LayoutRect {
                    x: saturate(i64::from(layout.rect.x) + dx),
                    y: saturate(i64::from(layout.rect.y) + dy),
                    ..layout.rect
                };
            }
            stack.extend(self.children.get(&id).into_iter().flatten());
        }
    }

    fn remove_subtree(&mut self, node: NodeId) {
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            self.layouts.remove(&id);
            stack.extend(self.children.get(&id).into_iter().flatten());
        }
    }
}

/// Cells from the anchor's start to the node's start on one axis. Worked in half cells so an
/// uneven center rounds toward the start.
fn along(axis: AnchorAxis, anchor_len: u16, own_len: u16) -> i64 {
    let edge = |edge: AnchorEdge, len: u16| match edge {
        AnchorEdge::Start => 0,
        AnchorEdge::Center => i64::from(len),
        AnchorEdge::End => 2 * i64::from(len),
    };
    (edge(axis.anchor, anchor_len) - edge(axis.own, own_len)).div_euclid(2) + i64::from(axis.offset)
}

fn saturate(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}
