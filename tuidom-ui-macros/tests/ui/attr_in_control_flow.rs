use tuidom_ui_macros::view;

fn main() {
    let _ = view! {
        if true { focusable: true }
    };
}
