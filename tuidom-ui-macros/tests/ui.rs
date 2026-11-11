//! Compile-fail tests for the diagnostics `view!` and `#[component]` emit themselves.
//!
//! Scoped to what the macros reject before emitting code — a misspelled event, a missing `in`,
//! an attribute with no element, a stray clause, an unknown `#[component]` option, and a
//! reactive body's closure reading a signal inside a read or its text hole that is not a name.
//! These abort expansion before any `::tuidom_ui::…` code is emitted, so the fixtures need only
//! this proc-macro crate to compile and their stderr is the macro's own wording spanned at the
//! user's token.
//!
//! Errors rustc raises about a *well-formed* expansion — a bool attribute given an integer, a
//! handler of the wrong arity — are spanned at the user's interpolated tokens by construction,
//! but their wording belongs to the compiler, so pinning it here would only break on a
//! toolchain bump.

#[test]
fn errors_point_at_the_users_tokens() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
