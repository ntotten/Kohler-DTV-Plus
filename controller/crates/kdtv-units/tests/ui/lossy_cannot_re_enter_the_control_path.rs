//! Converting Fx2 back to Celsius loses a step: Cx2 86 encodes to Fx2 218,
//! which converts back to Cx2 85. The inverse therefore returns a type no
//! constructor accepts, so a round-tripped value cannot become a setpoint.
use kdtv_units::{Fx2, ValveSetpoint, temp};

fn main() {
    let back = temp::fx2_to_lossy_cx2(Fx2::from_raw(218));
    let _ = ValveSetpoint::try_new(back);
}
