# Security Notes

Security posture of the DTV+ controller (K-99695), written for owners. The product is discontinued and unsupported, so this document's purpose is to help the people who own these systems understand and contain the risk.

**Scope:** production firmware 0.0.3.89 (the common production build). Methods: documentation review, source review of the RTOS web stack, and rate-limited read-only probing. No denial-of-service testing, no write-class endpoint testing.

---

## Summary

The controller is a 2012-era embedded device with **no authentication, no transport security, and an unsigned firmware path**, permanently frozen by a **decommissioned update server**. Treat it as a trusted-LAN-only device.

## Findings

### 1. No authentication or request-forgery protection on the entire API

Every HTTP endpoint — including hardware actuation and destructive resets — accepts unauthenticated requests from any LAN host. There is no credential, cookie, token, origin check, or CORS restriction. A web page open in any browser on the LAN can drive the API from any origin.

**Mitigation:** put the controller on a dedicated VLAN/SSID that only admin machines can reach; never port-forward it; never bridge it to guest networks.

### 2. The update pipeline is plaintext FTP against a decommissioned server

- Firmware downloads over cleartext FTP (port 21) with a hardcoded username (`ftpuser`), from folder `00` (or a hospitality folder). See [firmware-update.md](firmware-update.md).
- The configured server is decommissioned (NXDOMAIN as of 2026). Every DTV+ in the field is frozen at its shipped build.
- Because the flow is plaintext and DNS-directed, anyone controlling DNS on the LAN can capture the device's FTP credentials and control what it downloads. On your own network this is a legitimate research tool; on an untrusted network it is an attack.

### 3. Firmware upload without signature verification

`fileupload.cgi` + `unpack_bin.cgi` accept and apply firmware images from any LAN client. Validation is the bootloader's CRC32 + address-range check only — integrity, not authenticity. A corrupt image bricks the unit until the TFS recovery flow; a crafted image passing CRC would execute. This is also the feature that makes owner-controlled repair possible — see [repair/firmware-extraction.md](repair/firmware-extraction.md).

### 4. Fragile network stack (availability)

- The HTTP server supports **two concurrent sessions, total**; a third wedges it for ~20 seconds, and sustained bursting crashes the controller (manual power cycle required). Trivial LAN denial of service.
- A known RTCS memory leak kills the network task at roughly two months of uptime (`NETWORK_TASK_ABORT`, code 146). The weekly-reboot workaround also covers flash-filesystem degradation (code 103). See [troubleshooting/known-issues.md](../research/xagon0/docs/troubleshooting/known-issues.md).

### 5. Raw internals exposed over CGI

`edit_dt.cgi` (raw datatable read/write), `rpc.cgi` (internal RPC by index), `set_device.cgi`, `swapvalves.cgi`, and the reset family are all reachable unauthenticated. Several are persistent-damage class. The per-endpoint risk ratings are in [web-interface/cgi-endpoints.md](web-interface/cgi-endpoints.md).

### 6. Known-CVE posture of the platform

- The one applicable CVE, **CVE-2021-22680** ("BadAlloc" — integer overflow in MQX `mem_alloc` / `_lwmem_alloc` / `_partition`, MQX <= 5.1), needs an attacker-influenced allocation size. A source review of the network-facing MQX 3.8 code (HTTP request path, session allocator, FTP client response parsers) found **no such size reachable at the network edge**. See [repair/firmware-extraction.md](repair/firmware-extraction.md#ruled-out-with-reasons).
- The HCC Embedded CVEs found in searches (CVE-2020-25767, CVE-2021-31226/7/8, CVE-2021-31400/1, CVE-2021-36762) affect HCC's **InterNiche/NicheStack** TCP/IP — not present here. The HCC product in this device is the SafeFAT/SafeFlash filesystem. Do not conflate them.

### What the HTTP server does NOT allow (verified)

The web server's document root is a read-only TFS compiled into the firmware image, and MQX 3.8's `httpd` strips `/../` correctly, never percent-decodes the path, and binds the filesystem volume before path resolution — so **static-file path traversal to the SafeFAT volume (where the firmware lives) is not possible** through this server. No FTP or telnet listeners run. Details and evidence: [repair/firmware-extraction.md](repair/firmware-extraction.md).

---

## Owner checklist

| # | Action |
|---|---|
| 1 | Isolate the controller on its own VLAN; only admin hosts may reach it |
| 2 | Never expose it to the internet or port-forward it |
| 3 | Reboot weekly (a smart plug on a schedule suffices) to pre-empt the flash and network-stack failure modes |
| 4 | Automate against the API serialised, <= 1 request/second, failing closed |
| 5 | Run the update flow only on a LAN whose DNS you control, and never serve the device an unverified image |

---

## Notes

- Disclosure: not applicable — the product is discontinued and unsupported; this is community documentation for owners.
- Sources: public record (FCC exhibits, patents, Kohler product literature), open-source review, and read-only interaction with an owned device. No Kohler confidential material.
- Not affiliated with Kohler Co.
