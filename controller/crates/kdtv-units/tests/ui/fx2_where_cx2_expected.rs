//! Fx2 220 is 110 F. The same byte read as Cx2 asks a valve for 110 C, which is
//! more than twice the valve's own hardware ceiling. Range checking does not
//! catch it, because both values are in range for their own encoding. The types
//! must refuse it instead.
use kdtv_units::{Cx2, Fx2, ValveSetpoint};

fn main() {
    let steam_setpoint = Fx2::from_raw(220);
    // A valve setpoint takes Cx2. Handing it the steam encoding is the hazard.
    let _ = ValveSetpoint::try_new(steam_setpoint);

    // And there is no conversion to reach for either.
    let _: Fx2 = Cx2::from_raw(76).into();
}
