//! Compile-fail tests: the mistakes this crate exists to make impossible.
//!
//! A passing unit test proves the code does the right thing when called
//! correctly. These prove the wrong call **does not compile**, which is the
//! stronger claim and the one the safety argument rests on.
//!
//! Each case in `tests/ui/` is a small program that must be rejected, paired
//! with the exact error expected. The toolchain is pinned in
//! `rust-toolchain.toml`, so the messages are stable.

#[test]
fn the_encoding_split_cannot_be_crossed() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
