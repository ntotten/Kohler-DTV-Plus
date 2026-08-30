//! Compile-fail tests for the authority types.
//!
//! The claim these support is not "the kernel checks before granting" but "there
//! is no other way to obtain a grant, and an authority cannot be spent twice".
//! A unit test cannot make that claim; a program that fails to compile can.

#[test]
fn req_agent_safe_02_authority_cannot_be_forged_or_duplicated() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
