use tuidom_ui::{View, component, state, view};

#[component(reactive)]
fn First() -> View {
    let items = state(vec![1, 2, 3]);
    let first = items.first();
    view! { text(format!("{first:?}")) }
}

fn main() {}
