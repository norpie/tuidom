use tuidom_ui_macros::component;

#[component(reactive)]
fn Total(items: Vec<i32>, offset: i32) -> View {
    let total = derived!(items.iter().map(|i| i + offset).sum::<i32>());
    view! { text(total) }
}

fn main() {}
