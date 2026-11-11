use tuidom_ui::{Signal, View, component, state, view};

fn reset(count: Signal<i32>) {
    count.set(0);
}

#[component(reactive)]
fn Counter() -> View {
    let count = state(3);
    reset(count);
    view! { text(count) }
}

fn main() {}
