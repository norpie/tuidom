use tuidom_ui_macros::component;

#[component(reactive)]
fn Label(items: Vec<i32>) -> View {
    view! { text("{items.len()} items") }
}

fn main() {}
