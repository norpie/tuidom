use tuidom_ui_macros::view;

fn main() {
    let _ = view! {
        each i, item in [1, 2, 3] nope { text(item) }
    };
}
