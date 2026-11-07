use tuidom_ui_macros::view;

fn main() {
    let _ = view! {
        box { on_hover: move |_| {} }
    };
}
