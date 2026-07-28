use tuidom_ui_macros::component;

#[component]
fn Duplicated(children: Children, children: Children) -> View {
    view! { text("x") }
}

fn main() {}
