use tuidom_ui::{View, component, view};

#[component]
fn Counter(start: i32) -> View {
    view! { text(start.get()) }
}

fn main() {
    let _ = view! { Counter { bogus: 1 } };
}
