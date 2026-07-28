use tuidom_ui::{View, component, view};

// `Leaf` declares no `children` parameter, so giving it body nodes is an error.
#[component]
fn Leaf(label: i32) -> View {
    view! { text(label.get()) }
}

fn main() {
    let _ = view! { Leaf { label: 1, text("x") } };
}
