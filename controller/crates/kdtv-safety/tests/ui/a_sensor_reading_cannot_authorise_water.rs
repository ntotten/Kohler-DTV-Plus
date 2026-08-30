//! The independent temperature probe has no authority to open an outlet. Its
//! only output type is a SafetyEvent, and nothing in the workspace turns a
//! reading into a grant.
use kdtv_safety::{OpenGrant, RtdSample};
use kdtv_telemetry::Monotonic;
use kdtv_units::RawC;

fn main() {
    let sample = RtdSample { raw: RawC(38.0), fault_register: 0, at: Monotonic::from_nanos(0) };
    let _: OpenGrant = sample.into();
}
