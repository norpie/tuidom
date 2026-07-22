use tuidom_macros::style;

fn main() {
    let base = ();
    let _ = style! {
        color: white,
        ..base,
    };
}
