//! One valve bus: its port, its valve family, its master identity, its outlet
//! table and its instrumented outlet.

use crate::error::ConfigError;
use crate::port::PortPath;
use kdtv_proto::saturn::{MasterAddr, OutletMapping, OutletTable, ValveType};
use kdtv_units::{LinkKind, Slot, SlotSet, ZoneId};
use serde::Deserialize;

/// The valve families this service will accept in a configuration file.
///
/// `kdtv_proto`'s [`ValveType`] has four; the contract in
/// `deploy/kdtvd.toml` names two, and those two are the ones this installation
/// has and the test suite exercises. The other two — `Prompt2Port`, whose
/// bitmap no source gives, and `Prompt3FlowControl` — are absent by design:
/// there is no variant here that spells them, so a configuration cannot select
/// an outlet layout that has never been exercised.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConfiguredValve {
    /// Firmware type `0x06`. Outlets 0..5, masks `0x01`..`0x20`.
    #[serde(rename = "dtv-6-port")]
    Dtv6Port,
    /// Firmware type `0x1E`. Outlets 1..6, masks `0x04`..`0x80`.
    #[serde(rename = "prompt-3-port")]
    Prompt3Port,
}

impl ConfiguredValve {
    #[must_use]
    pub const fn valve_type(self) -> ValveType {
        match self {
            Self::Dtv6Port => ValveType::Dtv6Port,
            Self::Prompt3Port => ValveType::Prompt3Port,
        }
    }
}

/// One outlet, as the file spells it, after validation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutletConfig {
    /// The configuration slot — the only number the public API speaks.
    pub slot: Slot,
    /// The index `system_info.cgi`-shaped status reports this outlet under.
    pub status_index: u8,
    /// The outlet number in the valve family's own numbering.
    pub wire_outlet: u8,
    /// What it drives, for the operator and for the log. Not a wire value.
    pub name: String,
}

/// A validated zone.
///
/// The fields are private and the only constructor is
/// [`crate::ValidatedConfig::load`], so a `ZoneConfig` that exists has passed
/// every check in this module.
#[derive(Clone, Debug)]
pub struct ZoneConfig {
    id: ZoneId,
    port: PortPath,
    expected_valve: ConfiguredValve,
    master: MasterAddr,
    outlets: OutletTable,
    labels: Vec<OutletConfig>,
    instrumented_slot: Slot,
}

impl ZoneConfig {
    pub(crate) fn build(
        id: ZoneId,
        port: PortPath,
        expected_valve: ConfiguredValve,
        master_address: u8,
        outlets: Vec<OutletConfig>,
        instrumented_slot: u8,
    ) -> Result<Self, ConfigError> {
        let master = match master_address {
            0x00 => MasterAddr::Dtv,
            0x10 => MasterAddr::Prompt,
            value => {
                return Err(ConfigError::MasterAddress { zone: id, value });
            }
        };

        if outlets.is_empty() {
            return Err(ConfigError::NoOutlets { zone: id });
        }

        // `OutletTable::new` is the one implementation of the slot / status /
        // wire mapping and of its duplicate rules. This crate does not repeat
        // them; it attaches the zone to the refusal.
        let table = OutletTable::new(
            expected_valve.valve_type(),
            outlets.iter().map(|o| OutletMapping {
                slot: o.slot,
                status_index: o.status_index,
                wire_outlet: o.wire_outlet,
            }),
        )
        .map_err(|source| ConfigError::Outlets {
            zone: id,
            source: Box::new(source),
        })?;

        let configured = table.configured_slots();
        let instrumented = Slot::new(instrumented_slot)
            .ok()
            .filter(|s| configured.contains(*s));
        let Some(instrumented) = instrumented else {
            return Err(ConfigError::InstrumentedSlotNotConfigured {
                zone: id,
                slot: instrumented_slot,
                configured: ConfigError::slot_list(configured.iter()),
            });
        };

        Ok(Self {
            id,
            port,
            expected_valve,
            master,
            outlets: table,
            labels: outlets,
            instrumented_slot: instrumented,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ZoneId {
        self.id
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        LinkKind::Zone(self.id)
    }

    #[must_use]
    pub const fn port(&self) -> &PortPath {
        &self.port
    }

    /// The valve family the operator says is on this bus.
    ///
    /// **Expected, not established.** `kdtv_proto` derives the authoritative
    /// [`ValveType`] from the firmware type the valve reports at discovery,
    /// never from configuration, because a mis-configured family opens the wrong
    /// outlet silently. This value exists so discovery has something to
    /// contradict: a zone whose reported family differs from this one is a
    /// refusal, not a correction.
    #[must_use]
    pub const fn expected_valve(&self) -> ValveType {
        self.expected_valve.valve_type()
    }

    /// The master identity this link speaks as — `0x00` or `0x10`.
    ///
    /// Per-zone because the sources disagree over which one a Prompt 3 answers
    /// and this installation is exactly the irreconcilable case.
    /// `INVESTIGATIONS.md` I5, packet-capture question 1.
    #[must_use]
    pub const fn master(&self) -> MasterAddr {
        self.master
    }

    #[must_use]
    pub const fn outlets(&self) -> &OutletTable {
        &self.outlets
    }

    /// The configured slots, as a set.
    #[must_use]
    pub fn configured_slots(&self) -> SlotSet {
        self.outlets.configured_slots()
    }

    /// The outlet whose supply pipe carries the independent temperature probe.
    ///
    /// Continuous independent coverage exists for this outlet only; every other
    /// outlet is verified individually with an immersion probe at
    /// commissioning.
    #[must_use]
    pub const fn instrumented_slot(&self) -> Slot {
        self.instrumented_slot
    }

    /// The operator's label for a slot, if it has one.
    #[must_use]
    pub fn label(&self, slot: Slot) -> Option<&str> {
        self.labels
            .iter()
            .find(|o| o.slot == slot)
            .map(|o| o.name.as_str())
    }

    pub(crate) fn with_port(&self, port: PortPath) -> Self {
        Self {
            port,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    fn outlet(slot: u8, status_index: u8, wire_outlet: u8) -> OutletConfig {
        OutletConfig {
            slot: Slot::new(slot).unwrap(),
            status_index,
            wire_outlet,
            name: format!("outlet {slot}"),
        }
    }

    fn port() -> PortPath {
        PortPath::parse(
            "zones.zone1.port",
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
            Profile::Production,
        )
        .unwrap()
    }

    fn zone1() -> Result<ZoneConfig, ConfigError> {
        ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            (1u8..=5).map(|n| outlet(n, n, n - 1)).collect(),
            1,
        )
    }

    #[test]
    fn the_reference_zone_builds() {
        let z = zone1().unwrap();
        assert_eq!(z.id(), ZoneId::Zone1);
        assert_eq!(z.link(), LinkKind::Zone(ZoneId::Zone1));
        assert_eq!(z.expected_valve(), ValveType::Dtv6Port);
        assert_eq!(z.master(), MasterAddr::Dtv);
        assert_eq!(z.master().byte(), 0x00);
        assert_eq!(z.configured_slots().len(), 5);
        assert_eq!(z.instrumented_slot().get(), 1);
        assert_eq!(z.label(Slot::new(3).unwrap()), Some("outlet 3"));
        assert_eq!(z.outlets().valve(), ValveType::Dtv6Port);
    }

    #[test]
    fn the_prompt_master_address_is_the_other_accepted_value() {
        let z = ZoneConfig::build(
            ZoneId::Zone2,
            port(),
            ConfiguredValve::Prompt3Port,
            0x10,
            (1u8..=3).map(|n| outlet(n, n, n)).collect(),
            1,
        )
        .unwrap();
        assert_eq!(z.master(), MasterAddr::Prompt);
        assert_eq!(z.master().byte(), 0x10);
    }

    #[test]
    fn any_other_master_address_is_refused() {
        for value in [0x01u8, 0x03, 0x0F, 0x11, 0xFF] {
            let err = ZoneConfig::build(
                ZoneId::Zone1,
                port(),
                ConfiguredValve::Dtv6Port,
                value,
                vec![outlet(1, 1, 0)],
                1,
            )
            .unwrap_err();
            assert!(matches!(err, ConfigError::MasterAddress { .. }), "{value}");
            assert!(err.to_string().contains("master_address"));
        }
    }

    #[test]
    fn a_duplicate_slot_is_refused() {
        let err = ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            vec![outlet(1, 1, 0), outlet(1, 2, 1)],
            1,
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("zones.zone1.outlets"), "{text}");
        assert!(text.contains("twice"), "{text}");
    }

    #[test]
    fn a_duplicate_status_index_is_refused() {
        let err = ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            vec![outlet(1, 4, 0), outlet(2, 4, 1)],
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("status index 4"), "{err}");
    }

    #[test]
    fn a_duplicate_wire_outlet_is_refused() {
        let err = ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            vec![outlet(1, 1, 0), outlet(2, 2, 0)],
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("wire outlet 0"), "{err}");
    }

    /// A DTV 6-Port numbers its outlets 0..5 and a Prompt 3 numbers its 1..6.
    /// Wire outlet 0 exists on one family and not on the other, and wire outlet
    /// 6 the other way round.
    #[test]
    fn a_wire_outlet_the_family_does_not_have_is_refused() {
        let err = ZoneConfig::build(
            ZoneId::Zone2,
            port(),
            ConfiguredValve::Prompt3Port,
            0x00,
            vec![outlet(1, 1, 0)],
            1,
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("zones.zone2.outlets"), "{text}");
        assert!(text.contains("Prompt 3-Port"), "{text}");

        let err = ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            vec![outlet(1, 1, 6)],
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("DTV 6-Port"), "{err}");
    }

    #[test]
    fn an_empty_outlet_list_is_refused() {
        assert!(matches!(
            ZoneConfig::build(
                ZoneId::Zone1,
                port(),
                ConfiguredValve::Dtv6Port,
                0x00,
                vec![],
                1
            ),
            Err(ConfigError::NoOutlets { .. })
        ));
    }

    #[test]
    fn an_instrumented_slot_that_is_not_configured_is_refused() {
        // Slot 4 is inside 1..=6 but is not in this zone's table.
        let err = ZoneConfig::build(
            ZoneId::Zone1,
            port(),
            ConfiguredValve::Dtv6Port,
            0x00,
            vec![outlet(1, 1, 0), outlet(2, 2, 1)],
            4,
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("zones.zone1.instrumented_slot = 4"), "{text}");
        assert!(text.contains("configured: 1, 2"), "{text}");

        // And a slot outside 1..=6 at all, which never reaches the table.
        for bad in [0u8, 7, 255] {
            assert!(matches!(
                ZoneConfig::build(
                    ZoneId::Zone1,
                    port(),
                    ConfiguredValve::Dtv6Port,
                    0x00,
                    vec![outlet(1, 1, 0)],
                    bad
                ),
                Err(ConfigError::InstrumentedSlotNotConfigured { .. })
            ));
        }
    }

    #[test]
    fn the_configuration_names_only_the_two_families_the_contract_names() {
        #[derive(Deserialize, Debug)]
        struct W {
            valve: ConfiguredValve,
        }
        let a: W = toml::from_str(r#"valve = "dtv-6-port""#).unwrap();
        assert_eq!(a.valve.valve_type(), ValveType::Dtv6Port);
        let b: W = toml::from_str(r#"valve = "prompt-3-port""#).unwrap();
        assert_eq!(b.valve.valve_type(), ValveType::Prompt3Port);
        // Present in kdtv_proto, absent here: no variant spells them.
        assert!(toml::from_str::<W>(r#"valve = "prompt-2-port""#).is_err());
        assert!(toml::from_str::<W>(r#"valve = "prompt-3-flow-control""#).is_err());
    }
}
