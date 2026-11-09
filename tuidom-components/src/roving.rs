//! Composite controls that are one tab stop but focus their items one by one: each item is
//! focusable, so hovering it focuses it, and only the item that should take Tab is a stop.

use tuidom_ui::NodeRef;

/// The attribute naming an item's index within its control.
pub(crate) const ITEM: &str = "item";

/// Focus the child of `container` whose [`ITEM`] attribute is `index`.
pub(crate) fn focus_item(container: NodeRef, index: usize) {
    let (Some(doc), Some(node)) = (container.document(), container.node()) else {
        return;
    };
    let wanted = index.to_string();
    let item = doc.get_children(node).into_iter().find(|&child| {
        doc.get_attr(child, ITEM).ok().flatten().as_deref() == Some(wanted.as_str())
    });
    if let Some(item) = item
        && let Err(err) = doc.focus(item)
    {
        tracing::warn!(?item, "focusing an item failed: {err}");
    }
}
