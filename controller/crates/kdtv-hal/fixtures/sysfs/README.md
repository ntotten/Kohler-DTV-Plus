# Fixture sysfs trees

Directory trees that stand in for a machine, so the port resolver's refusals can
be exercised on an x86_64 CI runner with no converter attached. The layout is
documented in `src/sysfs.rs`; it is flattened relative to real sysfs, and a
`by-id` entry is a file containing the target node path rather than a symlink.

- `reference` — the configuration `deploy/kdtvd.toml` names: one Waveshare
  converter, three interfaces, no USB serial number, `ftdi_sio`, latency timer
  at the FTDI default of 16 ms. Resolves and hardens.
- `missing-interface` — the same converter with the third interface absent.
  Binding the steam link is refused: present or refuse to start, no degraded
  branch.
- `shared-serial` — two separate converters reporting the same USB serial
  number. Their udev `by-id` names collide, so a `by-id` binding no longer names
  a particular converter. Refused.
- `aliased` — two `by-id` names pointing at one device node. Two zones bound to
  them would drive the same bus. Refused.
- `latency-stuck` — a bridge that accepts the latency-timer write and keeps its
  old value. The extra `latency_timer.writes_stick` file is fixture-only; real
  sysfs has no such attribute.
- `non-ftdi` — a CP2102 bridge. `latency_timer` is FTDI-specific and this
  bridge's low-latency equivalent has not been established, so it is refused
  rather than opened unhardened.

What these trees prove is the resolver's decisions given an answer. They do not
prove that `RealSysfs` reads the answer correctly off a real kernel; that walk
is exercised only on the Pi.
