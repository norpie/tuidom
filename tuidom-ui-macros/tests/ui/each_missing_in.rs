use tuidom_ui_macros::view;

fn main() {
    let _ = view! {
        each i, item [1, 2, 3] { text(item) }
    };
}
