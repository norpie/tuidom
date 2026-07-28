use tuidom_ui::view;

// `Missing` is PascalCase, so `view!` dispatches it as a component — but no such component
// exists, so its generated props type is unresolved. The error points at `Missing`.
fn main() {
    let _ = view! { Missing {} };
}
