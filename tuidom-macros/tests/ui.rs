//! Compile-fail tests for the errors `style!` emits itself.
//!
//! Scoped to the macro's own diagnostics. Errors that rustc raises about the expansion —
//! an enum variant that does not exist, a value of the wrong type — are spanned at the
//! user's tokens by construction, but their wording belongs to the compiler, so pinning it
//! here would only break on a toolchain bump.

#[test]
fn errors_point_at_the_users_tokens() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
