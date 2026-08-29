# Kohler replacement controller shopping list

Verified: 2026-08-29. This is the purchasing companion to
[CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md).

Use manufacturer-direct stores and approved or authorized resellers. Avoid
marketplace inventory for the controller, storage, measurement tools, and
valve interface.

Raspberry Pi recommends its Approved Reseller network so buyers receive a
reputable product and local purchasing support. The links below use PiShop US
for Raspberry Pi hardware, B&H as an authorized SanDisk dealer, Waveshare
directly for the packaged valve interface, and ThermoWorks and Fluke directly
for instruments. The approximately $500 industrial PLC alternative is
intentionally excluded.

## Cart 1: PiShop US

PiShop US is the selected US Raspberry Pi Approved Reseller. These items were
shown in stock when checked.

| Qty | Item                                                     | Link                                                                                     | Buying note                                                     |
| --: | -------------------------------------------------------- | ---------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
|   1 | Raspberry Pi 4 Model B, 2 GB                             | [PiShop US](https://www.pishop.us/product/raspberry-pi-4-model-b-2gb/)                   | Dedicated valve controller, local API, logs, and wired network. |
|   1 | Official Raspberry Pi 15 W USB-C power supply, US, white | [PiShop US](https://www.pishop.us/product/raspberry-pi-15w-power-supply-us-white/)       | Independent 5.1 V / 3 A supply; never tap valve power.          |
|   1 | Passively cooled metal Raspberry Pi 4 case               | [PiShop US](https://www.pishop.us/product/pishop-premium-metal-case-for-raspberry-pi-4/) | Bench/service protection without a fan.                         |

The final installed dry-service enclosure may replace the Pi case after its
layout is approved.

## Cart 2: B&H Photo

| Qty | Item                                                       | Link                                                                                                                                                | Buying note                                                                         |
| --: | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
|   2 | SanDisk 64 GB High Endurance microSD, `SDSQQNR-064G-GN6IA` | [B&H, authorized dealer](https://www.bhphotovideo.com/c/product/1987067-REG/sandisk_sdsqqnr_064g_gn6ia_64gb_high_endurance_microsdhc.html/overview) | One installed and one imaged recovery spare. The exact manufacturer number matters. |

B&H listed the cards at $26.99 each and in stock when checked.

## Cart 3: Waveshare direct

The exact converter was not found at DigiKey, Mouser, or PiShop US. Buy it from
the manufacturer rather than a marketplace seller.

| Qty | Item                                                     | Purchase link                                                                 | Buying note                                                        |
| --: | -------------------------------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------ |
|   2 | Waveshare `USB TO RS485/422`, SKU `23949`               | [Waveshare direct](https://www.waveshare.com/usb-to-rs485-422.htm)           | Use one separately isolated converter per valve.                   |
|   2 | Included USB-A-to-USB-B cable, approximately 1.2 meters | [Included with SKU 23949](https://www.waveshare.com/wiki/USB_TO_RS485/422)   | One is included with each converter; order no duplicate USB cable. |

Confirm on arrival that the two units report distinct USB serial numbers.
Adapters in this class may ship with blank or duplicated serials; two identical
ones make a `by-id` symlink resolve both zones onto the same device, which the
controller's start check does not catch because the path resolves. On a
collision, bind by physical USB port path and label the ports.

Each converter was listed at $17.99 when checked, or approximately $35.98 for
both before shipping. Each includes isolated power and signal paths, automatic
direction control, protection circuitry, screw terminals, selectable 120-ohm
termination, and a DIN-rail enclosure.

The dual-channel SKU `27646` is $17.99 total, but Waveshare does not document
channel-to-channel galvanic isolation. It is not selected because two separate
converters provide one documented isolation barrier per valve for only about
$18 more.

## Independent temperature sensor

A permanent outlet temperature sensor read by the Pi, required by
[CONTROLLER-DESIGN.md](CONTROLLER-DESIGN.md). Also the instrument required by
[INVESTIGATIONS.md](../../INVESTIGATIONS.md) E5.

Select after the enclosure layout is known: clamp-on pipe probe or inline
fitting, depending on accessible pipe. Do not order with the rest.

## Passive capture equipment

Do not connect either bidirectional adapter in parallel with the operating
K-99695 for packet capture. Select a professional isolated analyzer with a
hardware listen-only mode or assemble a temporary isolated receiver whose
driver enable is physically fixed inactive. Capture equipment is not part of
the permanent controller and should not be ordered until that method is chosen.

Select a device that timestamps in hardware. A USB-serial front end quantises
arrival times at its latency-timer interval, 16 ms by default, which does not
resolve jitter on a 525 ms tick or a 320 ms response deadline. Use a logic
analyzer on the differential pair or an analyzer with hardware listen-only
mode.

## Instruments

| Qty | Item                                                                       | Source                                                                                               | Buying note                                                                                        |
| --: | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
|   1 | Therma K meter and fast-response probe kit with NIST-traceable certificate | [ThermoWorks direct](https://www.thermoworks.com/products/therma-k-kit)                              | Required for documented physical outlet-water measurements.                                        |
|   1 | Fluke 117 true-RMS multimeter                                              | [Fluke direct](https://www.fluke.com/en-us/product/electrical-testing/digital-multimeters/fluke-117) | Buy only if a suitable meter is not already available. The electrician handles mains measurements. |

Borrow or rent the oscilloscope and isolated differential probe. Do not buy a
cheap marketplace probe for this job.

## Do not order yet

- Valve mating connectors, pigtails, or replacement data cables: photograph
  and meter both installed connectors first.
- RS-485 cable, termination, and bias components: capture and measure the
  original bus and determine adapter-lead length before choosing parts.
- Ferrules, cable glands, terminal blocks, fuses, DIN rail, and enclosure:
  determine conductor gauges, adapter-lead length, and the reviewed layout
  first.
- Manual valve-power disconnect hardware: an electrician must identify valve
  voltage, receptacles, circuits, and GFCI arrangement first.
- Any additional protection component: the packaged adapter already includes
  isolation and transient protection; add nothing without an electrical review.

## Explicitly avoid

- The Unipi Patron or another approximately $500 PLC for this two-link system.
- Generic `MAX485`, `MAX3485`, or TTL-to-RS485 modules. Most are not isolated,
  and some default their transmitter active during boot.
- Bidirectional USB-to-RS485 adapters for passive capture. Hardware automatic
  direction control is not physically receive-only.
- Hobby relays, smart plugs, or inline cord switches in either valve's mains
  power path.
