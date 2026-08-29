//! `OpenGrant` is the right to open water. `SafetyKernel::authorize_open` is its
//! only source, so "what in this system can turn water on" is answered by
//! reading one function — which is only true if nothing else can build one.
use kdtv_safety::OpenGrant;
use kdtv_units::{BootId, CommandId, ZoneId};

fn main() {
    let _ = OpenGrant::new(ZoneId::Zone1, CommandId(1), BootId(1));
}
