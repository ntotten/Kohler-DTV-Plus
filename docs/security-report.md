# Security report — Kohler DTV+ (K-99695)

Security-relevant findings for the DTV+ system controller, written for
owners. The product is discontinued and unsupported; nothing here is
actionable by the vendor, so this document's purpose is to help the people
who own these systems understand and contain the risk — and to record the
facts that matter for repair.

**Scope:** K-99695-NA controller, production firmware 0.0.3.89 (the common
production build), plus the update pipeline and the product family's public
record. Methods: documentation review, source review of the RTOS web stack,
and rate-limited read-only probing of a production unit. No denial-of-service
testing, no write-class endpoint testing.

## Summary

The controller is a 2012-era embedded device with **no authentication, no
transport security, and an unsigned firmware path**, permanently frozen by a
**decommissioned update server**. It should be treated as a
trusted-LAN-only device: fine behind network segmentation, unsafe anywhere
else.

## Findings

### 1. No authentication or request forgery protection on the entire API

Every HTTP endpoint — including hardware actuation and destructive
resets — accepts unauthenticated requests from any LAN host. There is no
credential, cookie, token, origin check, or CORS restriction. A web page
open in any browser on the LAN can drive the API from any origin, including
the endpoints that physically turn on water.

**Mitigation (owners):** put the controller on a dedicated VLAN/SSID that
only your admin machines can reach; never port-forward it; never bridge it to
guest or IoT-general networks.

### 2. The update pipeline is plaintext FTP against a decommissioned server

- The controller resolves the update host via a hardcoded public resolver
  (8.8.8.8), then downloads firmware over **cleartext FTP (port 21)** with a
  hardcoded username (`ftpuser`), from folder `00` (or a hospitality
  folder). Manifest: `versions.txt`.
- The configured server (`pr0d3ct-upd.kohler.com`) now returns **NXDOMAIN**.
  Every DTV+ in the field is frozen at its shipped build (most: 0.0.3.89).
- Because the flow is plaintext and DNS-directed, **anyone controlling DNS
  on the LAN can capture the device's FTP credentials** and control what it
  downloads. On an owner's own network this is a legitimate repair/research
  tool; on an untrusted network it is an attack.

### 3. Firmware upload without signature verification

`fileupload.cgi` + `unpack_bin.cgi` accept and apply firmware images from any
LAN client. Validation is the bootloader's **CRC32 + address-range check
only** — integrity, not authenticity. A corrupt image bricks the unit until
the TFS recovery flow; a crafted image passing CRC would execute. This is
also the feature that makes owner-controlled repair possible.

### 4. Fragile network stack (availability)

- The HTTP server supports **two concurrent sessions, total**; a third
  wedges it for ~20 seconds, and sustained bursting crashes the controller
  (manual power cycle required). This is a trivial LAN denial of service.
- A known RTCS memory leak kills the network task at roughly two months of
  uptime (`NETWORK_TASK_ABORT`, code 146): the web UI disappears while the
  shower keeps working from the wall panel. A weekly reboot is the community
  workaround and also covers flash-filesystem degradation (code 103, ~1 week
  horizon).

### 5. Raw internals exposed over CGI

`edit_dt.cgi` (raw datatable read/write), `rpc.cgi` (internal RPC by index),
`set_device.cgi`, `swapvalves.cgi` and the reset family are all reachable
unauthenticated. Several are persistent-damage-class. The full rated list is
maintained upstream (see [reference/links.md](../research/reference-links.md)).

### 6. Known-CVE posture of the platform

- The stack (MQX 3.8 / RTCS) predates the public vulnerability era for this
  RTOS. The one applicable CVE, **CVE-2021-22680** ("BadAlloc" — integer
  overflow in MQX `mem_alloc`/`_lwmem_alloc`/`_partition`, MQX ≤ 5.1), is an
  allocator bug that needs an attacker-influenced allocation size; a source
  review of the network-facing MQX 3.8 code (HTTP request path, session
  allocator, FTP client response parsers) found **no such size reachable at
  the network edge**. Details: [research/firmware-extraction.md](repair/firmware-extraction-notes.md).
- The HCC Embedded CVEs that show up in searches (CVE-2020-25767,
  CVE-2021-31226/7/8, CVE-2021-31400/1, CVE-2021-36762) affect HCC's
  **InterNiche/NicheStack** TCP/IP — not present here. The HCC product in
  this device is the SafeFAT/SafeFlash filesystem. Don't conflate them.

### What the HTTP server does NOT allow (verified)

To save future researchers the effort: the web server's document root is a
read-only TFS compiled into the firmware image. Review of MQX 3.8's `httpd`
source confirms the URL sanitizer strips `/../` (correctly), the request path
is never percent-decoded, and the filesystem volume is bound before path
resolution — so **static-file path traversal to the SafeFAT volume (where the
firmware lives) is not possible** through this server. No FTP or telnet
listeners run. Verified empirically and in source; see
[research/firmware-extraction.md](repair/firmware-extraction-notes.md).

## Owner checklist

1. Isolate the controller on its own VLAN; only your admin host(s) may reach
   it.
2. Never expose it to the internet or port-forward it.
3. Reboot it weekly (a smart plug on a schedule is enough) to pre-empt the
   flash and network-stack failure modes.
4. If you automate against the API: serialise requests, ≤ 1/second, and fail
   closed.
5. If you must run the update flow for research, do it only on a LAN whose
   DNS you control, and never serve the device an image you have not
   verified (see [docs/firmware-and-updates.md](firmware-and-updates.md)).

## Notes

- Disclosure: not applicable — the product is discontinued and unsupported;
  this document is community documentation for owners.
- Nothing here was obtained from Kohler confidential material; sources are
  public (FCC exhibits, patents, Kohler's public product literature),
  open-source review, and read-only interaction with an owned device.
- Not affiliated with Kohler Co. Report corrections via the repo's issue
  tracker.
