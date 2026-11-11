use tuidom_ui_macros::component;

#[component(svelte)]
fn Counter(start: i32) -> View {
    view! { text("x") }
}

fn main() {}
