#[cfg(test)]
mod tests;

use std::collections::HashSet;

use tuidom::event::KeyCode;
use tuidom::style::{Color, FlexDirection, Style};
use tuidom_ui::{
    Callback, Children, Signal, View, component, derived, node_ref, state, view, virtual_list,
    virtual_list_ref,
};

use crate::list::{list_style, moved, row_style};

/// One node of a [`Tree`]: an `id` unique within the tree, the `label` shown, and its
/// `children`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeNode {
    /// Names the node in a tree's `expanded` and `selected`, so it must be unique.
    pub id: String,
    /// The text shown for the node.
    pub label: String,
    /// The nodes under this one, shown while it is expanded.
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// A node with no children.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        TreeNode {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
        }
    }

    /// Gives the node `children`.
    pub fn with_children(mut self, children: Vec<TreeNode>) -> Self {
        self.children = children;
        self
    }
}

/// A scrolling tree of `nodes`, one row per node shown. `expanded` holds the ids of the nodes
/// showing their children and `selected` the id of the selected node, both read and written.
///
/// Only the rows in view exist, so the tree can be large. A node with children is marked `▸`,
/// or `▾` while expanded, and each level is indented two cells. While the tree is focused, Up,
/// Down, PageUp, PageDown, Home and End move the selection over the rows shown, as in a
/// [`List`](crate::List). Right expands the selected node, or moves to its first child once
/// expanded; Left collapses it, or moves to its parent. Enter calls `on_activate` with the
/// selected id. A click selects a row, and a click on its marker also expands or collapses it.
/// The selected row is drawn like a list's.
///
/// `style` is merged over the tree's own and must bound its height, as a list's must.
#[component]
pub fn Tree(
    nodes: Vec<TreeNode>,
    expanded: Signal<HashSet<String>>,
    selected: Signal<Option<String>>,
    #[prop(default)] on_activate: Callback<String>,
    #[prop(default)] style: Style,
) -> View {
    let focused = state(false);
    let frame_ref = node_ref();
    let list = virtual_list_ref();
    let frame = derived(move || {
        let mut frame = Style::new();
        frame.flex_direction(FlexDirection::Column);
        frame.merge(&style.get());
        frame
    });
    let rows = derived(move || flatten(&nodes.get(), &expanded.get()));
    let current = move || {
        let id = selected.get();
        rows.get()
            .iter()
            .position(|row| Some(&row.id) == id.as_ref())
    };

    let select = move |index: usize| {
        if let Some(row) = rows.get().get(index) {
            selected.set(Some(row.id.clone()));
            list.scroll_into_view(index);
        }
    };
    let toggle = move |id: String| {
        expanded.update(|open| {
            if !open.remove(&id) {
                open.insert(id);
            }
        });
    };
    let page = move || usize::from(frame_ref.layout().map_or(1, |l| l.scrollport.height).max(1));

    let content = Children::new(move || {
        virtual_list(
            list_style(),
            1,
            2,
            move || rows.get().len(),
            move |i| {
                let row = move || rows.get().get(i).cloned().unwrap_or_default();
                view! {
                    box {
                        style: row_style(selected.get() == Some(row().id), focused.get(), 1),
                        on_click: move |_| {
                            select(i);
                            frame_ref.focus();
                        },
                        text("  ".repeat(row().depth)),
                        box {
                            on_click: move |_| {
                                let row = row();
                                if row.branch {
                                    toggle(row.id);
                                }
                            },
                            text(row().marker()) { style: marker() },
                        },
                        text(row().label),
                    }
                }
            },
        )
        .list_ref(list)
        .into()
    });

    view! {
        box {
            ref: frame_ref,
            focusable: true,
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_key_press: move |ev| {
                let rows = rows.get();
                let Some(last) = rows.len().checked_sub(1) else {
                    return;
                };
                let at = current().unwrap_or(0);
                let row = &rows[at];
                if let Some(next) = moved(ev.code, at, last, page()) {
                    select(next);
                } else {
                    match (ev.code, row.parent) {
                        (KeyCode::Right, _) if row.branch && !row.open => toggle(row.id.clone()),
                        (KeyCode::Right, _) if row.open => select(at + 1),
                        (KeyCode::Left, _) if row.open => toggle(row.id.clone()),
                        (KeyCode::Left, Some(parent)) => select(parent),
                        (KeyCode::Enter, _) => on_activate.call(row.id.clone()),
                        _ => return,
                    }
                }
                ev.prevent_default();
                ev.stop_propagation();
            },
            { content }
        }
    }
}

/// A node as a row: where it sits and whether it shows children.
#[derive(Debug, Clone, PartialEq, Default)]
struct Row {
    id: String,
    label: String,
    depth: usize,
    /// The row of the node's parent.
    parent: Option<usize>,
    /// Has children, so it can expand.
    branch: bool,
    open: bool,
}

impl Row {
    fn marker(&self) -> &'static str {
        match (self.branch, self.open) {
            (false, _) => "  ",
            (true, false) => "▸ ",
            (true, true) => "▾ ",
        }
    }
}

/// The rows shown: every root, and the children of every expanded node shown, depth first.
fn flatten(nodes: &[TreeNode], expanded: &HashSet<String>) -> Vec<Row> {
    fn walk(
        nodes: &[TreeNode],
        depth: usize,
        parent: Option<usize>,
        expanded: &HashSet<String>,
        rows: &mut Vec<Row>,
    ) {
        for node in nodes {
            let index = rows.len();
            let branch = !node.children.is_empty();
            let open = branch && expanded.contains(&node.id);
            rows.push(Row {
                id: node.id.clone(),
                label: node.label.clone(),
                depth,
                parent,
                branch,
                open,
            });
            if open {
                walk(&node.children, depth + 1, Some(index), expanded, rows);
            }
        }
    }
    let mut rows = Vec::new();
    walk(nodes, 0, None, expanded, &mut rows);
    rows
}

fn marker() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--muted-foreground"));
    style
}
