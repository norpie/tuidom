use tuidom_ui_macros::view;

// Children go in the body as nodes, not as a `children:` attribute — the two would fight over
// the one slot, so `view!` rejects the attribute form at the key.
fn main() {
    let _ = view! { Panel { children: whatever } };
}
