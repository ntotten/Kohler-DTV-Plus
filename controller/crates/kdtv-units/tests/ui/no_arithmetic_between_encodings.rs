//! The two encodings are not numbers in the same space, so they do not add,
//! compare or convert. `Cx2::to_fx2` is the only crossing, and `clippy.toml`
//! confines it to the steam encoder.
use kdtv_units::{Cx2, Fx2};

fn main() {
    let c = Cx2::from_raw(76);
    let f = Fx2::from_raw(200);
    let _ = c == f;
    let _ = c < f;
}
