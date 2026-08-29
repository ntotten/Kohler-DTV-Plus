//! A setpoint outside the clamp must be unconstructible, not merely rejected.
//! The field is private, so the only ways in are `try_new` and `clamped`.
use kdtv_units::{Cx2, Fx2, SteamSetpoint, ValveSetpoint};

fn main() {
    // 98 is the valve's hardware ceiling, far above the 85 comfort clamp.
    let _ = ValveSetpoint(Cx2::from_raw(98));
    let _ = SteamSetpoint(Fx2::from_raw(255));
}
