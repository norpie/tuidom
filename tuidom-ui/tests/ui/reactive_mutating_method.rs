use tuidom_ui::{View, component, state, view};

#[component(reactive)]
fn List() -> View {
    let items = state(vec![1, 2, 3]);
    let add = move || items.push(4);
    view! { box { on_click: move || add() } }
}

fn main() {}
