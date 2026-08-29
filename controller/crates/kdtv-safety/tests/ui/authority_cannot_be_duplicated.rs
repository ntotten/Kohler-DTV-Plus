//! The authority types are move-only. A `Clone` would turn "the operator asked
//! for this" into "the operator asked for this, and we may replay it whenever
//! we like".
use kdtv_safety::{OperatorAck, StartAuthorization};
use kdtv_units::{BootId, CommandId};

fn main() {
    let _ = OperatorAck::issue(CommandId(2)).clone();
    let _ = StartAuthorization::issue(BootId(1), CommandId(1)).clone();
}
