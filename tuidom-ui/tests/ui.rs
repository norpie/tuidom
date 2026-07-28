//! Compile-fail tests for the `#[component]` misuse that only surfaces once an expansion is
//! type-checked — an unknown prop, a missing required prop, stray children on a component that
//! takes none, and a PascalCase name that is not a component.
//!
//! Unlike the parse-time cases in `tuidom-ui-macros` (which abort before emitting any
//! `::tuidom_ui::…` code), these expand to a real props struct and typestate builder, so they
//! need the whole crate in scope to reproduce. The *wording* is rustc's, not ours; what these
//! fixtures pin is that the error lands on the user's own token — the component name or the prop
//! key — which is the macro's span choices doing their job. Because the wording is the compiler's,
//! a toolchain bump can shift it; regenerate with `TRYBUILD=overwrite cargo test -p tuidom-ui`.

#[test]
fn component_misuse_points_at_the_call_site() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
