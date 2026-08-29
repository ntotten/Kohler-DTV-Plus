//! Golden frames with evidence tiers, and the hash that binds a fixture set to
//! a [`TransmitAuthority`](crate::gate::TransmitAuthority).
//!
//! # What a fixture is
//!
//! One frame, its raw bytes, where the bytes came from, and what this crate's
//! decoder makes of them. The point is the *where from*: this repository marks
//! every claim with an evidence tier (`docs/system-specification.md`), and a
//! frame is no different from a sentence in a document. [`Provenance`] is that
//! tier, made into a type:
//!
//! | Variant | Tier | Means |
//! | --- | --- | --- |
//! | [`Provenance::Captured`] | `[A]` | Measured on this installation, with the capture file, byte offset, phase and scenario that produced it |
//! | [`Provenance::Documented`] | `[C]` | Read out of third-party reverse engineering, **unverified against this hardware** |
//!
//! **Today every fixture is `[C]`.** That is the honest state of the project:
//! the K-99695's Saturn buses have never been probed, and no DTV+ bus has ever
//! been captured here at all. [`FixtureSet::documented_only`] returns the whole
//! set, and the transmit gate reads that and stays shut.
//!
//! # A fixture is not a frame
//!
//! [`Fixture::bytes`] hands out a `&[u8]`. There is no `From<Fixture>` for
//! [`SaturnFrame`](crate::saturn::SaturnFrame) or
//! [`DtvFrame`](crate::dtv::DtvFrame), and there never should be: fixture bytes
//! are evidence, and the only thing this system may transmit is what an encoder
//! built from an allowlisted operation. What the fixtures do instead is check
//! the encoders — every transmit fixture is asserted, in this module's tests,
//! to be byte-for-byte what the encoder produces for the operation it names.
//!
//! # Where a fixture set comes from
//!
//! Outside this crate there is exactly one: [`FixtureSet::embedded`], compiled
//! in from `controller/fixtures/*.json` with `include_dir`. Parsing is
//! crate-private, so no caller can hand
//! [`TransmitAuthority::resolve`](crate::gate::TransmitAuthority::resolve) a
//! fabricated evidence base and talk the gate open.
//!
//! # The set hash
//!
//! [`FixtureSet::sha256`] is a digest over the parsed content of the whole set
//! — ids, links, directions, bytes, provenance and decoded forms, each
//! length-prefixed, in id order. It is over content and not over file bytes, so
//! reformatting the JSON does not move it, and it covers provenance, so
//! promoting one fixture from `[C]` to `[A]` does. Configuration pins it; see
//! [`crate::gate`].

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use include_dir::{Dir, include_dir};
use kdtv_units::LinkKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::dtv::SteamOpKind;
use crate::saturn::SaturnOpKind;

/// `controller/fixtures/`. Only `.json` files are fixture files; the directory
/// also holds the fixture sysfs trees `kdtv-hal`'s tests walk, which are not
/// frames and are skipped by extension.
static FIXTURE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../fixtures");

/// Domain separator, so this digest cannot collide with some other SHA-256 in
/// the workspace and so a format change can be versioned rather than silently
/// reinterpreted.
const DIGEST_DOMAIN: &[u8] = b"kdtv-proto/fixtures/v1";

/// The evidence tier a fixture carries, in this repository's notation.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum EvidenceTier {
    /// `[A]` — ours, measured on this hardware.
    A,
    /// `[C]` — reverse-engineered by a third party, unverified here.
    C,
}

impl fmt::Display for EvidenceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::A => f.write_str("[A]"),
            Self::C => f.write_str("[C]"),
        }
    }
}

/// Where a fixture's bytes came from.
///
/// The two variants are the two tiers, and there is no third: a frame is either
/// something this project measured or something it read. In particular there is
/// no "derived" variant — a frame built from field definitions rather than
/// quoted from a capture is still only as good as the document, so it is
/// [`Provenance::Documented`] and says so in its `section`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Tier `[A]`: measured on this installation.
    Captured {
        /// The capture this came out of, repository-relative, under
        /// `research/diagnostics/` with the date in the filename.
        file: String,
        /// Byte offset of the frame's first byte within that capture.
        offset: u64,
        /// The commissioning phase that produced the capture.
        phase: u8,
        /// The scenario within that phase.
        scenario: u8,
        /// SHA-256 of the capture file, lowercase hex, so a re-recorded capture
        /// under the same filename does not pass as the reviewed one.
        sha256: String,
    },
    /// Tier `[C]`: third-party reverse engineering, unverified against this
    /// hardware. See `research/xagon0/PROVENANCE.md`.
    ///
    /// **Cannot satisfy
    /// [`RealBusAttested`](crate::gate::TransmitScope::RealBusAttested).**
    Documented {
        /// Repository-relative path of the document.
        source: String,
        /// The section, example or table within it, quoted closely enough that
        /// a reviewer can find it.
        section: String,
    },
}

impl Provenance {
    #[must_use]
    pub const fn tier(&self) -> EvidenceTier {
        match self {
            Self::Captured { .. } => EvidenceTier::A,
            Self::Documented { .. } => EvidenceTier::C,
        }
    }

    /// True only for tier `[A]`. This is the predicate the transmit gate reads.
    #[must_use]
    pub const fn is_captured(&self) -> bool {
        matches!(self, Self::Captured { .. })
    }
}

/// Which way along the link the frame travelled.
///
/// One type for both protocols. `saturn::Direction` and `dtv::DtvDirection`
/// name the peer — a valve, a device — and a fixture only needs to say whether
/// this master sent it or received it, which is the question the gate and the
/// replay harness ask.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureDirection {
    /// Master to valve, or master to steam device. These are the frames the
    /// transmit gate cares about, because these are the frames this system
    /// would put on a bus.
    ToDevice,
    /// Valve to master, or steam device to master. Decoder material.
    FromDevice,
}

impl fmt::Display for FixtureDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ToDevice => f.write_str("to-device"),
            Self::FromDevice => f.write_str("from-device"),
        }
    }
}

/// A fixture's identifier.
///
/// Lowercase, dot-separated, ASCII: `saturn.set_outlets`,
/// `steam.response.status`. Validated on construction so a typo in a JSON file
/// is a load error rather than a fixture that silently answers no lookup.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixtureId(Box<str>);

impl FixtureId {
    /// Maximum length. Long enough for `saturn.response.firmware_type`, short
    /// enough that an id stays a name.
    pub const MAX_LEN: usize = 64;

    pub fn new(raw: impl Into<String>) -> Result<Self, FixtureError> {
        let raw: String = raw.into();
        let ok = !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && raw
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
            && !raw.starts_with('.')
            && !raw.ends_with('.')
            && !raw.contains("..");
        if ok {
            Ok(Self(raw.into_boxed_str()))
        } else {
            Err(FixtureError::BadId { raw })
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FixtureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for FixtureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FixtureId({})", &self.0)
    }
}

impl PartialEq<str> for FixtureId {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

/// One recorded frame.
///
/// Private fields and no public constructor, for the same reason the frame
/// types have none: the evidence base is not something a caller assembles.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fixture {
    id: FixtureId,
    link: LinkKind,
    direction: FixtureDirection,
    bytes: Vec<u8>,
    provenance: Provenance,
    decoded: serde_json::Value,
}

impl Fixture {
    #[must_use]
    pub const fn id(&self) -> &FixtureId {
        &self.id
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }

    #[must_use]
    pub const fn direction(&self) -> FixtureDirection {
        self.direction
    }

    /// The raw frame, sync bytes and checksum included.
    ///
    /// Evidence, not a frame: there is no route from these bytes to anything
    /// the link layer will transmit.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    #[must_use]
    pub const fn tier(&self) -> EvidenceTier {
        self.provenance.tier()
    }

    /// This crate's decoder's reading of [`Fixture::bytes`], recorded in the
    /// file so a change in the decoder shows up as a diff against evidence
    /// rather than as a quietly different interpretation. The tests in this
    /// module re-decode every fixture and compare.
    #[must_use]
    pub const fn decoded(&self) -> &serde_json::Value {
        &self.decoded
    }

    /// A tier `[A]` fixture that was never measured, for testing the *granting*
    /// half of the transmit gate.
    ///
    /// `#[cfg(test)]`, and crate-private: nothing outside this crate's own
    /// tests can mint attested evidence.
    #[cfg(test)]
    pub(crate) fn synthetic_captured(
        id: FixtureId,
        link: LinkKind,
        direction: FixtureDirection,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            id,
            link,
            direction,
            bytes,
            provenance: Provenance::Captured {
                file: "synthetic".to_owned(),
                offset: 0,
                phase: 1,
                scenario: 1,
                sha256: hex::encode([0u8; 32]),
            },
            decoded: serde_json::Value::Null,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_provenance_for_test(&mut self, provenance: Provenance) {
        self.provenance = provenance;
    }
}

/// The wire form of a fixture. Private, so [`Fixture`] itself has no
/// `Deserialize` and cannot be conjured from a `serde` input anywhere else.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFixture {
    id: String,
    link: LinkKind,
    direction: FixtureDirection,
    /// Lowercase hex, no separators. Hex rather than an array of numbers
    /// because a frame is read as hex in every capture tool and every document
    /// this project cites.
    bytes: String,
    provenance: Provenance,
    decoded: serde_json::Value,
}

/// The compiled-in evidence base.
///
/// Ordered by id, so the digest is deterministic and a listing reads like a
/// table of contents.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FixtureSet {
    fixtures: Vec<Fixture>,
    sha256: [u8; 32],
}

impl FixtureSet {
    /// The fixtures compiled into this binary.
    ///
    /// Parsed once, on first use. If the embedded JSON does not parse the set
    /// is **empty**, whose digest is all-zero and matches no configured pin, so
    /// the failure closes the transmit gate rather than opening it.
    /// [`FixtureSet::embedded_result`] reports the error, and
    /// [`the_embedded_fixtures_parse`](self#tests) asserts there is none.
    ///
    /// [`the_embedded_fixtures_parse`]: self#tests
    #[must_use]
    pub fn embedded() -> &'static Self {
        static EMPTY: FixtureSet = FixtureSet {
            fixtures: Vec::new(),
            sha256: [0u8; 32],
        };
        match Self::embedded_result() {
            Ok(set) => set,
            Err(_) => &EMPTY,
        }
    }

    /// [`FixtureSet::embedded`], with the load error if there was one.
    pub fn embedded_result() -> Result<&'static Self, &'static FixtureError> {
        static EMBEDDED: OnceLock<Result<FixtureSet, FixtureError>> = OnceLock::new();
        EMBEDDED.get_or_init(Self::load_embedded).as_ref()
    }

    fn load_embedded() -> Result<Self, FixtureError> {
        let mut raw: Vec<(String, RawFixture)> = Vec::new();
        let mut files = json_files(&FIXTURE_DIR);
        files.sort_by_key(|f| f.path());

        for file in files {
            let name = file.path().display().to_string();
            let parsed: Vec<RawFixture> =
                serde_json::from_slice(file.contents()).map_err(|e| FixtureError::Json {
                    file: name.clone(),
                    reason: e.to_string(),
                })?;
            for r in parsed {
                raw.push((name.clone(), r));
            }
        }

        let mut fixtures = Vec::with_capacity(raw.len());
        for (file, r) in raw {
            let id = FixtureId::new(r.id)?;
            let bytes = hex::decode(&r.bytes).map_err(|e| FixtureError::Hex {
                id: id.to_string(),
                file,
                reason: e.to_string(),
            })?;
            fixtures.push(Fixture {
                id,
                link: r.link,
                direction: r.direction,
                bytes,
                provenance: r.provenance,
                decoded: r.decoded,
            });
        }

        Self::from_fixtures(fixtures)
    }

    /// Crate-private. The public surface offers [`FixtureSet::embedded`] and
    /// nothing else, so a caller outside `kdtv-proto` cannot assemble an
    /// evidence base of its own and present it to the transmit gate.
    pub(crate) fn from_fixtures(mut fixtures: Vec<Fixture>) -> Result<Self, FixtureError> {
        fixtures.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in fixtures.windows(2) {
            if let [a, b] = pair
                && a.id == b.id
            {
                return Err(FixtureError::DuplicateId {
                    id: a.id.to_string(),
                });
            }
        }
        let sha256 = digest(&fixtures)?;
        Ok(Self { fixtures, sha256 })
    }

    /// SHA-256 over the whole set. See the module docs for what it covers.
    #[must_use]
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub fn sha256_hex(&self) -> String {
        hex::encode(self.sha256)
    }

    /// Lookup by id. `None` for an id this set does not carry, which for a
    /// required transmit fixture is itself a gate failure.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Fixture> {
        self.fixtures
            .binary_search_by(|f| f.id.as_str().cmp(id))
            .ok()
            .and_then(|i| self.fixtures.get(i))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.fixtures.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fixtures.is_empty()
    }

    /// Every fixture, in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Fixture> {
        self.fixtures.iter()
    }

    /// Every fixture still at tier `[C]` — that is, every frame this project
    /// believes but has not seen.
    ///
    /// **Today this returns the entire set.** It is the list the transmit gate
    /// prints when it refuses, and the worklist Phase 1 capture works through.
    #[must_use]
    pub fn documented_only(&self) -> Vec<&FixtureId> {
        self.fixtures
            .iter()
            .filter(|f| !f.provenance.is_captured())
            .map(|f| &f.id)
            .collect()
    }

    /// Every fixture at tier `[A]`. Today, empty.
    #[must_use]
    pub fn captured_only(&self) -> Vec<&FixtureId> {
        self.fixtures
            .iter()
            .filter(|f| f.provenance.is_captured())
            .map(|f| &f.id)
            .collect()
    }

    /// How many fixtures sit at each tier. For the boot log, so a support
    /// transcript records the evidence base in one line.
    #[must_use]
    pub fn tier_counts(&self) -> BTreeMap<EvidenceTier, usize> {
        let mut counts = BTreeMap::new();
        for f in &self.fixtures {
            *counts.entry(f.tier()).or_insert(0) += 1;
        }
        counts
    }
}

impl<'a> IntoIterator for &'a FixtureSet {
    type Item = &'a Fixture;
    type IntoIter = std::slice::Iter<'a, Fixture>;

    fn into_iter(self) -> Self::IntoIter {
        self.fixtures.iter()
    }
}

/// Every `.json` file in the embedded directory, at any depth.
///
/// An explicit walk rather than `include_dir`'s glob feature, so the crate does
/// not pull a glob engine in to match one extension. Anything that is not
/// `.json` is skipped: `controller/fixtures/` also holds the fixture sysfs
/// trees `kdtv-hal`'s tests walk, and those are not frames.
fn json_files(root: &'static Dir<'static>) -> Vec<&'static include_dir::File<'static>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in dir.entries() {
            match entry {
                include_dir::DirEntry::Dir(d) => stack.push(d),
                include_dir::DirEntry::File(f) => {
                    if f.path().extension().is_some_and(|e| e == "json") {
                        out.push(f);
                    }
                }
            }
        }
    }
    out
}

/// The canonical digest. Every field is length-prefixed, so no two different
/// sets can feed the hasher the same byte stream.
fn digest(fixtures: &[Fixture]) -> Result<[u8; 32], FixtureError> {
    let mut h = Sha256::new();
    h.update(DIGEST_DOMAIN);
    for f in fixtures {
        field(&mut h, f.id.as_str().as_bytes())?;
        field(&mut h, f.link.to_string().as_bytes())?;
        field(&mut h, &[direction_tag(f.direction)])?;
        field(&mut h, &f.bytes)?;
        match &f.provenance {
            Provenance::Captured {
                file,
                offset,
                phase,
                scenario,
                sha256,
            } => {
                field(&mut h, b"captured")?;
                field(&mut h, file.as_bytes())?;
                field(&mut h, &offset.to_le_bytes())?;
                field(&mut h, &[*phase, *scenario])?;
                field(&mut h, sha256.as_bytes())?;
            }
            Provenance::Documented { source, section } => {
                field(&mut h, b"documented")?;
                field(&mut h, source.as_bytes())?;
                field(&mut h, section.as_bytes())?;
            }
        }
        // `serde_json::Map` is a `BTreeMap` in this build, so its serialisation
        // is key-sorted and therefore canonical.
        let decoded = serde_json::to_string(&f.decoded).map_err(|e| FixtureError::Json {
            file: f.id.to_string(),
            reason: e.to_string(),
        })?;
        field(&mut h, decoded.as_bytes())?;
    }
    Ok(h.finalize().into())
}

fn field(h: &mut Sha256, bytes: &[u8]) -> Result<(), FixtureError> {
    let len =
        u32::try_from(bytes.len()).map_err(|_| FixtureError::FieldTooLong { len: bytes.len() })?;
    h.update(len.to_le_bytes());
    h.update(bytes);
    Ok(())
}

const fn direction_tag(d: FixtureDirection) -> u8 {
    match d {
        FixtureDirection::ToDevice => 0,
        FixtureDirection::FromDevice => 1,
    }
}

/// Why a fixture set would not load.
///
/// A load failure leaves [`FixtureSet::embedded`] empty, which closes the
/// transmit gate. There is no variant that degrades to a partial set: half an
/// evidence base is not evidence.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("fixture file {file} is not a JSON array of fixtures: {reason}")]
    Json {
        /// The file, relative to `controller/fixtures/`.
        file: String,
        /// The parser's own message.
        reason: String,
    },

    #[error("fixture {id} in {file} has malformed hex bytes: {reason}")]
    Hex {
        /// The fixture's id.
        id: String,
        /// The file it was found in.
        file: String,
        /// The decoder's own message.
        reason: String,
    },

    #[error(
        "{raw:?} is not a fixture id: lowercase ASCII, digits, '.' and '_', \
         at most {max} characters, no leading, trailing or doubled '.'",
        max = FixtureId::MAX_LEN,
    )]
    BadId {
        /// The id as written.
        raw: String,
    },

    #[error("two fixtures share the id {id}")]
    DuplicateId {
        /// The repeated id.
        id: String,
    },

    #[error("a fixture field is {len} bytes, above the digest's 4 GiB field limit")]
    FieldTooLong {
        /// The offending length.
        len: usize,
    },
}

/// The fixture id for one Saturn operation.
///
/// An explicit table rather than a derived name: it is exhaustive over
/// [`SaturnOpKind`], so adding an operation to the allowlist fails to compile
/// until a fixture id is chosen for it, and then fails
/// [`the_gate_is_closed_in_the_committed_state`](crate::gate) until a fixture
/// exists. Adding a way to move water therefore cannot be done without writing
/// down what it looks like on the wire.
#[must_use]
pub const fn saturn_op_fixture_id(kind: SaturnOpKind) -> &'static str {
    match kind {
        SaturnOpKind::AllOff => "saturn.all_off",
        SaturnOpKind::SetOutlets => "saturn.set_outlets",
        SaturnOpKind::SetTemperature => "saturn.set_temperature",
        SaturnOpKind::Pause => "saturn.pause",
        SaturnOpKind::Resume => "saturn.resume",
        SaturnOpKind::ReadFirmwareVersion => "saturn.read_firmware_version",
        SaturnOpKind::ReadFirmwareType => "saturn.read_firmware_type",
        SaturnOpKind::ReadOutlets => "saturn.read_outlets",
        SaturnOpKind::ReadTemperature => "saturn.read_temperature",
        SaturnOpKind::ReadFlow => "saturn.read_flow",
        SaturnOpKind::ReadFaults => "saturn.read_faults",
        SaturnOpKind::ReadCalibration => "saturn.read_calibration",
        SaturnOpKind::ReadConfiguration => "saturn.read_configuration",
        SaturnOpKind::ReadSerialNumber => "saturn.read_serial_number",
        SaturnOpKind::ReadGenericOutlets => "saturn.read_generic_outlets",
        SaturnOpKind::ReadExtendedStatus => "saturn.read_extended_status",
        SaturnOpKind::ReadDiagnostics => "saturn.read_diagnostics",
        SaturnOpKind::AddressEnquiry => "saturn.address_enquiry",
        SaturnOpKind::AddressAllocate => "saturn.address_allocate",
        SaturnOpKind::AddressClear => "saturn.address_clear",
    }
}

/// The fixture id for one steam operation. Exhaustive over [`SteamOpKind`], for
/// the same reason as [`saturn_op_fixture_id`].
#[must_use]
pub const fn steam_op_fixture_id(kind: SteamOpKind) -> &'static str {
    match kind {
        SteamOpKind::Start => "steam.start",
        SteamOpKind::Stop => "steam.stop",
        SteamOpKind::SetTemperature => "steam.set_temperature",
        SteamOpKind::SetDuration => "steam.set_duration",
        SteamOpKind::ReadStatus => "steam.read_status",
        SteamOpKind::ClearFaults => "steam.clear_faults",
        SteamOpKind::AddressOpportunity => "steam.address_opportunity",
        SteamOpKind::AssignAddress => "steam.assign_address",
    }
}

/// Every fixture id a real bus needs at tier `[A]`: one per allowlisted
/// operation on either protocol.
///
/// Built from `SaturnOpKind::ALL` and `SteamOpKind::ALL` rather than from a
/// literal list, so the requirement tracks the allowlist automatically. Response
/// fixtures are deliberately not in here — the gate is about what this system
/// transmits.
pub fn required_transmit_ids() -> impl Iterator<Item = &'static str> {
    SaturnOpKind::ALL
        .iter()
        .copied()
        .map(saturn_op_fixture_id)
        .chain(SteamOpKind::ALL.iter().copied().map(steam_op_fixture_id))
}

#[cfg(test)]
mod tests {

    /// Lets the fixture builder encode an outlet write.
    ///
    /// Built from the encoder's own link rather than pinned to a zone: an
    /// authority names one zone and authorises only that zone, and the fixture
    /// set covers both.
    #[derive(Debug)]
    struct FixtureAuthority(kdtv_units::ZoneId);
    impl kdtv_units::OpenAuthority for FixtureAuthority {
        fn authorised_zone(&self) -> kdtv_units::ZoneId {
            self.0
        }
    }
    use super::*;
    use crate::dtv::{self, DevAddr, DiscoveryStep, SteamOp, SteamOpState};
    use crate::gate::TransmitAuthority;
    use crate::saturn::{
        self, DiscoveryToken, LinkPhase, MasterAddr, OutletMapping, OutletTable, PrimaryFlags,
        SaturnOp, ValveAddr, ValveType,
    };
    use kdtv_units::{
        Cx2, LinkKind, Slot, SlotSet, SteamMinutes, SteamSetpoint, ValveSetpoint, ZoneId,
    };
    use serde_json::json;

    fn auth() -> TransmitAuthority {
        TransmitAuthority::emulator_only(FixtureSet::embedded())
    }

    fn zone1() -> saturn::Encoder {
        let table = OutletTable::new(
            ValveType::Dtv6Port,
            (1u8..=5).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n - 1,
            }),
        )
        .unwrap();
        saturn::Encoder::new(
            &auth(),
            LinkKind::Zone(ZoneId::Zone1),
            MasterAddr::Dtv,
            table,
        )
    }

    fn zone2() -> saturn::Encoder {
        let table = OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        saturn::Encoder::new(
            &auth(),
            LinkKind::Zone(ZoneId::Zone2),
            MasterAddr::Dtv,
            table,
        )
    }

    fn slots(ns: &[u8]) -> SlotSet {
        ns.iter().map(|n| Slot::new(*n).unwrap()).collect()
    }

    fn steam_pair() -> (SteamSetpoint, SteamMinutes) {
        (
            SteamSetpoint::try_new(SteamSetpoint::FACTORY_DEFAULT).unwrap(),
            SteamMinutes::default(),
        )
    }

    /// The parameters each transmit fixture was recorded with.
    ///
    /// This is the table that makes a fixture checkable: the encoder is run
    /// with exactly these arguments and the bytes must match the file. A
    /// fixture nobody can regenerate is an assertion, not evidence.
    fn saturn_cases() -> Vec<(&'static str, saturn::Encoder, SaturnOp)> {
        let temp = ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap();
        vec![
            ("saturn.all_off", zone2(), SaturnOp::AllOff),
            (
                "saturn.set_outlets",
                zone2(),
                SaturnOp::SetOutlets {
                    slots: slots(&[1]),
                    flags: PrimaryFlags::CAPTURED,
                },
            ),
            (
                "saturn.set_temperature",
                zone1(),
                SaturnOp::SetTemperature(temp),
            ),
            ("saturn.pause", zone2(), SaturnOp::Pause),
            ("saturn.resume", zone2(), SaturnOp::Resume),
            (
                "saturn.read_firmware_version",
                zone1(),
                SaturnOp::ReadFirmwareVersion,
            ),
            (
                "saturn.read_firmware_type",
                zone1(),
                SaturnOp::ReadFirmwareType,
            ),
            ("saturn.read_outlets", zone1(), SaturnOp::ReadOutlets),
            (
                "saturn.read_temperature",
                zone1(),
                SaturnOp::ReadTemperature,
            ),
            ("saturn.read_flow", zone1(), SaturnOp::ReadFlow),
            ("saturn.read_faults", zone1(), SaturnOp::ReadFaults),
            (
                "saturn.read_calibration",
                zone1(),
                SaturnOp::ReadCalibration,
            ),
            (
                "saturn.read_configuration",
                zone1(),
                SaturnOp::ReadConfiguration,
            ),
            (
                "saturn.read_serial_number",
                zone1(),
                SaturnOp::ReadSerialNumber,
            ),
            (
                "saturn.read_generic_outlets",
                zone1(),
                SaturnOp::ReadGenericOutlets,
            ),
            (
                "saturn.read_extended_status",
                zone1(),
                SaturnOp::ReadExtendedStatus,
            ),
            (
                "saturn.read_diagnostics",
                zone1(),
                SaturnOp::ReadDiagnostics,
            ),
            ("saturn.address_enquiry", zone1(), SaturnOp::AddressEnquiry),
            (
                "saturn.address_allocate",
                zone1(),
                SaturnOp::AddressAllocate(ValveAddr::new(0x03).unwrap()),
            ),
            ("saturn.address_clear", zone1(), SaturnOp::AddressClear),
        ]
    }

    fn steam_cases() -> Vec<(&'static str, SteamOp)> {
        let (temp, minutes) = steam_pair();
        vec![
            ("steam.start", SteamOp::Start { temp, minutes }),
            ("steam.stop", SteamOp::Stop { temp, minutes }),
            (
                "steam.set_temperature",
                SteamOp::SetTemperature {
                    temp,
                    minutes,
                    state: SteamOpState::Off,
                },
            ),
            (
                "steam.set_duration",
                SteamOp::SetDuration {
                    temp,
                    minutes,
                    state: SteamOpState::Off,
                },
            ),
            ("steam.read_status", SteamOp::ReadStatus),
            ("steam.clear_faults", SteamOp::ClearFaults),
            (
                "steam.address_opportunity",
                SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
            ),
            (
                "steam.assign_address",
                SteamOp::Discovery(DiscoveryStep::AssignAddress(DevAddr::REFERENCE)),
            ),
        ]
    }

    fn encode_saturn(e: &saturn::Encoder, op: &SaturnOp) -> Vec<u8> {
        let kind = op.kind();
        let (phase, token) = if kind.is_address_management() {
            (
                LinkPhase::Discovery,
                Some(DiscoveryToken::mint(e.link(), LinkPhase::Discovery).unwrap()),
            )
        } else {
            (LinkPhase::ReadyOff, None)
        };
        e.encode(
            ValveAddr::new(0x03).unwrap(),
            op,
            phase,
            token.as_ref(),
            e.link()
                .zone()
                .map(FixtureAuthority)
                .as_ref()
                .map(|a| -> &dyn kdtv_units::OpenAuthority { a }),
        )
        .unwrap()
        .bytes()
        .to_vec()
    }

    fn encode_steam(op: &SteamOp) -> Vec<u8> {
        let e = dtv::SteamEncoder::new(&auth());
        let kind = op.kind();
        let (phase, token) = if kind.is_discovery() {
            (
                LinkPhase::Discovery,
                Some(DiscoveryToken::mint(LinkKind::Steam, LinkPhase::Discovery).unwrap()),
            )
        } else {
            (LinkPhase::ReadyOff, None)
        };
        e.encode(DevAddr::REFERENCE, op, phase, token.as_ref())
            .unwrap()
            .bytes()
            .to_vec()
    }

    /// The decoder's reading of a Saturn frame, in the shape the fixture files
    /// record.
    fn decode_saturn_json(bytes: &[u8]) -> serde_json::Value {
        let mut rx = saturn::RxBuffer::new();
        rx.extend(bytes);
        let d = saturn::decode(&mut rx, &saturn::Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        json!({
            "address": d.address,
            "control": d.control.0,
            "data": hex::encode(d.data.as_slice()),
            "direction": format!("{:?}", d.inferred_direction),
        })
    }

    fn decode_dtv_json(bytes: &[u8]) -> serde_json::Value {
        let d = dtv::decode_frame(bytes).unwrap();
        json!({
            "dest": d.dest,
            "src": d.src,
            "cmd": d.cmd,
            "payload": hex::encode(d.payload.as_slice()),
            "direction": format!("{:?}", d.direction),
        })
    }

    // ---- The embedded set --------------------------------------------------

    #[test]
    fn the_embedded_fixtures_parse() {
        let set = FixtureSet::embedded_result().expect("controller/fixtures/*.json must parse");
        assert!(!set.is_empty());
    }

    /// **The honest state of the project, asserted.** Every frame this crate
    /// carries was read out of a document; none was measured here. When this
    /// test starts failing, Phase 1 has landed — and the failure is the prompt
    /// to update the gate configuration in the same commit.
    #[test]
    fn today_every_fixture_is_tier_c() {
        let set = FixtureSet::embedded();
        assert_eq!(set.documented_only().len(), set.len());
        assert_eq!(set.captured_only(), Vec::<&FixtureId>::new());
        assert_eq!(set.tier_counts().get(&EvidenceTier::A), None);
        assert_eq!(set.tier_counts().get(&EvidenceTier::C), Some(&set.len()));
        for f in set {
            assert_eq!(f.tier(), EvidenceTier::C, "{}", f.id());
            assert!(!f.provenance().is_captured(), "{}", f.id());
        }
    }

    /// Every documented fixture names a real file and a findable place in it.
    /// A provenance of "somewhere in the docs" is not provenance.
    #[test]
    fn every_provenance_names_a_source_and_a_section() {
        for f in FixtureSet::embedded() {
            let Provenance::Documented { source, section } = f.provenance() else {
                panic!("{} is not Documented", f.id());
            };
            assert!(
                std::path::Path::new(source)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("md")),
                "{}: {source}",
                f.id()
            );
            assert!(
                source.starts_with("research/") || source.starts_with("docs/"),
                "{}: {source}",
                f.id()
            );
            assert!(section.len() >= 3, "{}: {section:?}", f.id());
        }
    }

    /// The allowlist and the evidence base line up in both directions: every
    /// operation has a fixture, and no transmit fixture describes an operation
    /// that is not in the allowlist.
    #[test]
    fn every_allowlisted_operation_has_a_fixture() {
        let set = FixtureSet::embedded();
        assert_eq!(required_transmit_ids().count(), 28);
        for id in required_transmit_ids() {
            let f = set.get(id).unwrap_or_else(|| panic!("no fixture {id}"));
            assert_eq!(f.direction(), FixtureDirection::ToDevice, "{id}");
        }

        let required: Vec<&str> = required_transmit_ids().collect();
        for f in set {
            if f.direction() == FixtureDirection::ToDevice {
                assert!(
                    required.contains(&f.id().as_str()),
                    "{} is a transmit fixture for no allowlisted operation",
                    f.id()
                );
            }
        }
    }

    /// **The fixtures check the encoders.** Every transmit fixture is exactly
    /// what the encoder produces for the operation it names, given the
    /// parameters in [`saturn_cases`] and [`steam_cases`].
    #[test]
    fn every_transmit_fixture_is_what_the_encoder_produces() {
        let set = FixtureSet::embedded();
        let mut seen = 0;

        for (id, e, op) in saturn_cases() {
            let f = set.get(id).unwrap_or_else(|| panic!("no fixture {id}"));
            assert_eq!(f.bytes(), encode_saturn(&e, &op).as_slice(), "{id}");
            assert_eq!(f.link(), e.link(), "{id}");
            assert_eq!(f.decoded(), &decode_saturn_json(f.bytes()), "{id}");
            seen += 1;
        }

        for (id, op) in steam_cases() {
            let f = set.get(id).unwrap_or_else(|| panic!("no fixture {id}"));
            assert_eq!(f.bytes(), encode_steam(&op).as_slice(), "{id}");
            assert_eq!(f.link(), LinkKind::Steam, "{id}");
            assert_eq!(f.decoded(), &decode_dtv_json(f.bytes()), "{id}");
            seen += 1;
        }

        assert_eq!(seen, required_transmit_ids().count());
    }

    /// The response fixtures are the decoder's material: they are frames this
    /// system receives and must never send, and no encoder can produce them.
    #[test]
    fn every_response_fixture_decodes_to_what_it_records() {
        let set = FixtureSet::embedded();
        let mut seen = 0;
        for f in set {
            if f.direction() != FixtureDirection::FromDevice {
                continue;
            }
            let decoded = match f.link() {
                LinkKind::Zone(_) => decode_saturn_json(f.bytes()),
                LinkKind::Steam => decode_dtv_json(f.bytes()),
            };
            assert_eq!(f.decoded(), &decoded, "{}", f.id());
            seen += 1;
        }
        assert!(seen >= 4, "expected response fixtures, found {seen}");
    }

    /// `CHK-02`, the proof case, is in the evidence base: `AA 55 00 02 01 1E DF`
    /// validates only with the sync bytes excluded from the checksum. Including
    /// them yields `0xE0`.
    #[test]
    fn the_checksum_proof_case_is_a_fixture() {
        let f = FixtureSet::embedded()
            .get("saturn.response.firmware_type")
            .unwrap();
        assert_eq!(f.bytes(), &[0xAA, 0x55, 0x00, 0x02, 0x01, 0x1E, 0xDF]);
        assert_eq!(saturn::checksum(0x00, 0x02, 0x01, &[0x1E]), 0xDF);
        assert_eq!(
            saturn::checksum(0xAA, 0x55, 0x00, &[0x02, 0x01, 0x1E]),
            0xE0
        );
    }

    /// `CHK-04`: the source prints checksums `0xA1` and `0xAE` for two of its
    /// own examples, and both are stale. The corrected `0xFB` and `0xB3` are
    /// what the fixtures carry.
    #[test]
    fn the_corrected_checksums_are_what_the_fixtures_record() {
        let set = FixtureSet::embedded();
        let fw = set.get("saturn.read_firmware_type").unwrap();
        assert_eq!(fw.bytes(), &[0xAA, 0x55, 0x03, 0x02, 0x00, 0xFB]);
        assert_ne!(fw.bytes().last(), Some(&0xA1));

        let clear = set.get("saturn.address_clear").unwrap();
        assert_eq!(clear.bytes(), &[0xAA, 0x55, 0x0F, 0x3A, 0x01, 0x03, 0xB3]);
        assert_ne!(clear.bytes().last(), Some(&0xAE));
    }

    /// The one captured outlet-open frame in the sources, and the one place
    /// where the Prompt 3 bitmap differs from the DTV+ one: outlet 1 is mask
    /// `0x04`. `OUT-01`.
    #[test]
    fn the_outlet_write_fixture_is_the_prompt_3_mask() {
        let f = FixtureSet::embedded().get("saturn.set_outlets").unwrap();
        assert_eq!(f.bytes(), &[0xAA, 0x55, 0x03, 0x87, 0x02, 0x04, 0x00, 0x70]);
        assert_eq!(f.link(), LinkKind::Zone(ZoneId::Zone2));
    }

    /// `CORRECTIONS.md` item 1. The power-clean byte is a payload value, not an
    /// opcode, so the scan that proves it unreachable belongs on the fixtures
    /// too: no frame this system would transmit carries `0xCC` where the
    /// operation state goes.
    #[test]
    fn no_transmit_fixture_carries_the_power_clean_state_byte() {
        for (id, op) in steam_cases() {
            let f = FixtureSet::embedded().get(id).unwrap();
            let Some(offset) = dtv::state_byte_offset(op.kind()) else {
                continue;
            };
            assert_ne!(f.bytes().get(offset), Some(&0xCC), "{id}");
        }
    }

    /// No fixture carries a control byte from the Saturn denied list in its
    /// `CONTROL` field. Field-level, not a whole-frame byte scan: a checksum
    /// can legitimately be any byte, including `0xF4`.
    #[test]
    fn no_transmit_fixture_carries_a_denied_control_byte() {
        let denied = saturn::denied_control_bytes();
        for (id, ..) in saturn_cases() {
            let f = FixtureSet::embedded().get(id).unwrap();
            let control = f.bytes().get(3).copied().unwrap();
            assert!(!denied.contains(&control), "{id} carries {control:#04X}");
        }
    }

    // ---- Ids ---------------------------------------------------------------

    #[test]
    fn ids_are_validated() {
        assert!(FixtureId::new("saturn.set_outlets").is_ok());
        assert!(FixtureId::new("steam.response.status").is_ok());
        for bad in [
            "",
            "Saturn.SetOutlets",
            ".leading",
            "trailing.",
            "double..dot",
            "has space",
            "has-dash",
        ] {
            assert!(FixtureId::new(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(FixtureId::new("a".repeat(FixtureId::MAX_LEN)).is_ok());
        assert!(FixtureId::new("a".repeat(FixtureId::MAX_LEN + 1)).is_err());
    }

    #[test]
    fn every_generated_id_is_a_valid_id() {
        for id in required_transmit_ids() {
            assert_eq!(FixtureId::new(id).unwrap().as_str(), id);
        }
        for f in FixtureSet::embedded() {
            assert_eq!(FixtureId::new(f.id().as_str()).unwrap(), *f.id());
        }
    }

    #[test]
    fn the_op_id_tables_are_injective() {
        let mut ids: Vec<&str> = required_transmit_ids().collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let one = || {
            Fixture::synthetic_captured(
                FixtureId::new("saturn.all_off").unwrap(),
                LinkKind::Steam,
                FixtureDirection::ToDevice,
                vec![0x00],
            )
        };
        assert!(matches!(
            FixtureSet::from_fixtures(vec![one(), one()]),
            Err(FixtureError::DuplicateId { .. })
        ));
    }

    #[test]
    fn lookup_is_by_id_and_misses_are_none() {
        let set = FixtureSet::embedded();
        assert_eq!(
            set.get("saturn.read_faults").map(|f| f.id().as_str()),
            Some("saturn.read_faults")
        );
        assert!(set.get("saturn.write_calibration").is_none());
        assert!(set.get("").is_none());
    }

    #[test]
    fn the_set_is_ordered_by_id() {
        let ids: Vec<&str> = FixtureSet::embedded()
            .iter()
            .map(|f| f.id().as_str())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    // ---- The digest --------------------------------------------------------

    #[test]
    fn the_set_hash_is_stable_across_calls() {
        let a = FixtureSet::embedded().sha256();
        let b = FixtureSet::embedded().sha256();
        assert_eq!(a, b);
        assert_ne!(a, [0u8; 32]);
        assert_eq!(FixtureSet::embedded().sha256_hex().len(), 64);
    }

    /// Promoting a fixture's provenance moves the hash. This is the mechanism
    /// that makes opening the transmit gate a reviewable, dated act: the pin in
    /// configuration stops matching the moment the evidence changes.
    #[test]
    fn the_hash_covers_provenance() {
        let mut fixtures: Vec<Fixture> = FixtureSet::embedded().iter().cloned().collect();
        let before = FixtureSet::from_fixtures(fixtures.clone())
            .unwrap()
            .sha256();
        fixtures
            .first_mut()
            .unwrap()
            .set_provenance_for_test(Provenance::Captured {
                file: "research/diagnostics/2026-09-01-saturn.bin".to_owned(),
                offset: 4096,
                phase: 1,
                scenario: 2,
                sha256: hex::encode([1u8; 32]),
            });
        let after = FixtureSet::from_fixtures(fixtures).unwrap().sha256();
        assert_ne!(before, after);
    }

    #[test]
    fn the_hash_covers_bytes_and_ids_and_direction() {
        let base = |bytes: Vec<u8>, id: &str, dir| {
            FixtureSet::from_fixtures(vec![Fixture::synthetic_captured(
                FixtureId::new(id).unwrap(),
                LinkKind::Steam,
                dir,
                bytes,
            )])
            .unwrap()
            .sha256()
        };
        let a = base(vec![0x01], "steam.start", FixtureDirection::ToDevice);
        assert_ne!(
            a,
            base(vec![0x02], "steam.start", FixtureDirection::ToDevice)
        );
        assert_ne!(
            a,
            base(vec![0x01], "steam.stop", FixtureDirection::ToDevice)
        );
        assert_ne!(
            a,
            base(vec![0x01], "steam.start", FixtureDirection::FromDevice)
        );
    }

    /// Length prefixing, not concatenation: two fixtures whose fields run
    /// together the same way must still hash differently.
    #[test]
    fn the_digest_is_unambiguous_across_field_boundaries() {
        let one = |id: &str, bytes: Vec<u8>| {
            FixtureSet::from_fixtures(vec![Fixture::synthetic_captured(
                FixtureId::new(id).unwrap(),
                LinkKind::Steam,
                FixtureDirection::ToDevice,
                bytes,
            )])
            .unwrap()
            .sha256()
        };
        assert_ne!(one("ab.c", vec![0xAA]), one("ab.cd", vec![]));
    }

    #[test]
    fn the_empty_sets_hash_is_not_the_all_zero_pin() {
        // FixtureSet::embedded()'s failure fallback uses an all-zero hash on
        // purpose, so a broken evidence base matches no configured pin. A
        // genuinely empty set hashes to something else entirely.
        let empty = FixtureSet::from_fixtures(Vec::new()).unwrap();
        assert_ne!(empty.sha256(), [0u8; 32]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.documented_only(), Vec::<&FixtureId>::new());
    }

    // ---- Types -------------------------------------------------------------

    #[test]
    fn the_tiers_print_in_the_repositorys_notation() {
        assert_eq!(EvidenceTier::A.to_string(), "[A]");
        assert_eq!(EvidenceTier::C.to_string(), "[C]");
        assert_eq!(FixtureDirection::ToDevice.to_string(), "to-device");
        assert_eq!(FixtureDirection::FromDevice.to_string(), "from-device");
        assert_eq!(
            FixtureId::new("steam.start").unwrap().to_string(),
            "steam.start"
        );
    }

    #[test]
    fn provenance_round_trips_through_json_in_both_tiers() {
        for p in [
            Provenance::Documented {
                source: "research/xagon0/docs/protocols/saturn-protocol.md".to_owned(),
                section: "Example 1".to_owned(),
            },
            Provenance::Captured {
                file: "research/diagnostics/2026-09-01-saturn.bin".to_owned(),
                offset: 12,
                phase: 1,
                scenario: 3,
                sha256: hex::encode([2u8; 32]),
            },
        ] {
            let text = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<Provenance>(&text).unwrap(), p);
        }
        assert_eq!(
            serde_json::to_string(&Provenance::Documented {
                source: "s".to_owned(),
                section: "x".to_owned()
            })
            .unwrap(),
            r#"{"documented":{"source":"s","section":"x"}}"#
        );
    }

    /// A fixture file with a key nobody reads is a mistake, not a comment.
    #[test]
    fn unknown_fixture_keys_are_rejected() {
        let good = r#"[{"id":"steam.start","link":"steam","direction":"to_device",
            "bytes":"8803","provenance":{"documented":{"source":"a","section":"b"}},
            "decoded":null}]"#;
        assert!(serde_json::from_str::<Vec<RawFixture>>(good).is_ok());
        let bad = r#"[{"id":"steam.start","link":"steam","direction":"to_device",
            "bytes":"8803","provenance":{"documented":{"source":"a","section":"b"}},
            "decoded":null,"note":"hi"}]"#;
        assert!(serde_json::from_str::<Vec<RawFixture>>(bad).is_err());
    }

    /// A fixture is evidence, not a frame. There is no conversion, so the only
    /// way to transmit a fixture's bytes is to encode the operation that
    /// produces them — which is the point of
    /// [`every_transmit_fixture_is_what_the_encoder_produces`].
    ///
    /// [`every_transmit_fixture_is_what_the_encoder_produces`]: self#tests
    #[test]
    fn a_fixture_exposes_bytes_and_nothing_transmittable() {
        let f = FixtureSet::embedded().get("saturn.all_off").unwrap();
        let _bytes: &[u8] = f.bytes();
        assert_eq!(f.link(), LinkKind::Zone(ZoneId::Zone2));
        assert_eq!(f.direction(), FixtureDirection::ToDevice);
    }
}
