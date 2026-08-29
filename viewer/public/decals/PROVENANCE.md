# Decal artwork

Everything under this directory is **artwork we made**, not manufacturer
material. It is the opposite case to [`../models/PROVENANCE.md`](../models/PROVENANCE.md),
which holds third-party CAD we did not author and may not be free to
redistribute.

A decal is a flat image pinned to a measured face of a model. The geometry it
sits on is Kohler's; the picture on it is ours. That distinction matters enough
to keep the two directories apart, because the licensing answer is different for
each and because a viewer looking at a rendered part cannot tell which is which.

Decal records — anchors, fits, provenance notes — live in
[`../../src/catalog/decals.json`](../../src/catalog/decals.json). This file
covers the image files only.

## kohler-dtv-plus/k-99693-shower-dark.svg

|          |                                                                                              |
| -------- | -------------------------------------------------------------------------------------------- |
| Depicts  | The **replacement** interface built in this repository (`app/`), "shower" screen, dark theme |
| Authored | 2026-08-03, for this repository                                                              |
| Format   | SVG, self-contained: no external fonts, no embedded rasters, no scripts                      |
| viewBox  | `0 0 814.677 1310.691` — **exactly ten units per millimetre** of the K-99693 faceplate       |
| Rendered | Rasterised in-browser at 14 px/mm, giving 1141 × 1835 on an 81.47 × 131.07 mm face           |

**This is not Kohler's user interface.** The K-99693's real display shows
Kohler's own DTV+ software, which this drawing does not attempt to reproduce.
The _content_ is our replacement UI. Nothing in it should be read as a depiction
of the shipped product's software.

**The physical arrangement is copied from the real product**, from Kohler's own
product photography: portrait glass, an inset LCD occupying the upper three
quarters, and three capacitive buttons printed on the glass below it — power,
temperature down in blue, temperature up in red. That much is how the device
actually looks.

**Orientation is from the K-99694 bracket drawing, not from the mesh.** The
bracket is 84 × 143 mm portrait with the wiring boss at the bottom, which puts
the product's vertical along the CAD's X axis and product-down at +X — the end
carrying the raised connector block. The faceplate itself is a blank symmetric
rectangle and gives no cue either way, so this is documented evidence rather
than a measurement.

**The layout is a redraw, not a capture.** The elements are the same as the
running app — temperature, target, four outlets, the action bar, the connection
strip — but sized to the glass, so pixel positions here do not correspond to
pixel positions in the app.

**Why the viewBox is what it is.** Ten units per millimetre means a rectangle
200 units wide is 20 mm wide on the physical part, so editing the file is
dimensioned work. It also makes the artwork's aspect ratio equal the face's by
construction — and `npm run verify` fails if the two ever drift more than 1%
apart, rather than stretching the drawing to fit and saying nothing.

**Two constraints that are easy to break.** The file is rasterised inside an
`<img>`, which is a sandboxed XML document:

1. **No external references.** A webfont, a linked raster or a script silently
   does not load. Fonts are a system stack for this reason.
2. **No double hyphen inside a comment.** XML forbids it, and one occurrence
   makes the entire file fail to parse with nothing on screen to explain why.
   This bit us once already — the section rules in the header used to be dashes.

## kohler-dtv-plus/k-99693-shower-dark.png

|           |                                                                |
| --------- | -------------------------------------------------------------- |
| Depicts   | The same replacement interface, captured unretouched           |
| Source    | `research/screenshots/dark-02-shower.png`, captured 2026-07-26 |
| Size      | 1120 × 1800 px (portrait, aspect 0.622)                        |
| Copied on | 2026-08-03                                                     |

A verbatim copy of the screenshot, kept as a second decal on the same anchor.
Its 1120 × 1800 aspect is 0.6222 against the faceplate's 0.6216 — **0.11%
apart**, so it fills the real glass with no visible distortion. That is not
luck: the app's UI was laid out to the proportions of the physical device.

Keeping it alongside the drawn SVG is a standing check on that agreement. If
either the app's layout or the faceplate measurement drifts, `npm run verify`
will refuse this decal on aspect grounds rather than quietly squashing it.

## Licensing

Both files are our own work and carry the repository's licence. Neither
contains Kohler artwork, iconography or trade dress.
