//! An authorisation is consumed by the call it authorises, so it cannot be
//! presented a second time. This is why `SafetyKernel::authorize_open` takes it
//! by value — clippy asks for the type to be `Copy` instead, and that is exactly
//! what must not happen.
//!
//! Kept in its own file: a name-resolution error anywhere in a program aborts
//! before borrow-checking runs, so pairing this with the no-`Clone` case would
//! have silently proved only the other one.
use kdtv_safety::StartAuthorization;
use kdtv_units::{BootId, CommandId};

fn spend(_a: StartAuthorization) {}

fn main() {
    let auth = StartAuthorization::issue(BootId(1), CommandId(1));
    spend(auth);
    spend(auth);
}
