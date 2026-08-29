# Vendored model assets

Everything under this directory is **third-party manufacturer CAD**. It is not
our work, and it is vendored here only so the viewer has something to load
without a network fetch.

## kohler-dtv-plus/99693-P.obj

|                 |                                                                                                                               |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Part            | Kohler K-99693 DTV+ Digital Interface (wall control unit)                                                                     |
| Format          | Wavefront OBJ, ASCII                                                                                                          |
| Producer        | `3ds Max Wavefront OBJ Exporter v0.97b`, per the file's own header                                                            |
| Authored        | 2014-03-24 14:09:43, per the file's own header                                                                                |
| Geometry        | 2392 vertices, 2301 face records, 9146 vertex normals, one group `99693`                                                      |
| Triangulated    | 4544 triangles (the face records are largely quads)                                                                           |
| Materials       | None. No `mtllib`, no `usemtl`, no UVs.                                                                                       |
| Watertight      | **No.** 224 unshared edges — an open surface, not a solid.                                                                    |
| Downloaded from | Kohler's own product page — <https://www.kohler.com/en/products/showers/shop-shower-trims-valves/dtv-digital-interface-99693> |
| Staged at       | `E:\proj-med\build-661-diag-kohler-shower\kohler\kohler-digital-interface-99693\99693-P.obj`                                  |
| Copied on       | 2026-07-27                                                                                                                    |

**Source.** This file came from Kohler's public product page for the K-99693,
linked above, which publishes the part's CAD in several formats alongside the
spec sheet and manuals. It was not extracted from a third-party mirror, a CAD
marketplace, or a scrape. The `E:\proj-med\...` path is only where it was staged
locally after download.

**Verbatim copy.** The bytes here are identical to the source file; nothing was
re-exported, decimated or re-oriented. The viewer applies its unit and axis
transforms at load and export time instead, so the vendored asset stays
byte-comparable against whatever Kohler publishes.

### Units and orientation

The file declares neither, as OBJ has no facility to. Both were **established by
measurement, not assumed**:

| Axis | Mesh extent | Spec sheet         | Reading |
| ---- | ----------- | ------------------ | ------- |
| X    | 5.259       | 5-1/4 in (5.250)   | width   |
| Y    | 1.214       | 1-3/16 in (1.1875) | depth   |
| Z    | 3.310       | 3-5/16 in (3.3125) | height  |

So the file is in **inches**, with **Z up** and Y as depth — the usual 3ds Max
and mechanical-CAD convention. This is recorded as `sourceUnit: "in"` and
`sourceUpAxis: "z"` in `src/catalog/catalog.json`, and it is what makes the STL
export trustworthy: millimetres are a flat ×25.4 with no rotation at all.

Measured through the app's own export path, the envelope comes out at
**133.59 × 30.84 × 84.07 mm** against a published 133.35 × 30.16 × 84.14 mm:
+0.24 mm on width, +0.68 mm on depth, −0.07 mm on height.

The depth is the outlier. That is plausibly a trim-ring or mounting-boss detail
included in the CAD but excluded from the published depth. **It has not been
checked against the physical part** — treat the depth as the least certain of
the three.

### The mesh is not watertight as published

`npm run verify` reports **222 boundary edges across 11 open loops**, plus **2
non-manifold T-junction seams** where four triangles share a 0.03 mm edge. The
published CAD is an open surface, not a closed solid.

All 11 loops are closed and 8 of them are exactly planar, which is what makes
them safely cappable. The app's repair pass (`src/core/repair.ts`) closes the
mesh completely:

|                    | As published              | Repaired                   |
| ------------------ | ------------------------- | -------------------------- |
| Triangles          | 4,544                     | 4,736                      |
| Boundary edges     | 222                       | 0                          |
| Non-manifold edges | 2                         | 0                          |
| Watertight         | no                        | yes                        |
| Envelope           | 133.59 × 30.84 × 84.07 mm | **identical to 0.0001 mm** |

The envelope invariant is enforced by the verify gate, not just asserted: a
repair that moved the outer surface would be a dimensional error on the exact
faces a toolpath is cut against, so the gate fails if any axis drifts by more
than 0.0001 mm.

Both meshes are downloadable. The repaired one is the right choice for Fusion
360, CAM and slicing; the as-published one exists so Kohler's geometry can be
inspected unaltered.

### It is a hollow shell with no internal structure

Enclosed volume after capping is **190.30 cm³** against a 346 cm³ bounding box,
and the depth analysis finds no geometry at all between the front bezel
(Y ≈ −6 mm) and the rear plate (Y ≈ +13 to +15 mm) beyond the side walls.

This is a **visualization model**. It does not contain the PCB, the
wire-to-board connector, internal ribs, bosses or fasteners. It is a reliable
guide to the part's _outside_ and no guide whatsoever to what sits behind a
given point on the rear face. Anyone planning to cut into the real assembly
needs to establish clearances from the physical part, not from this file.

### Licensing — read before publishing

**Kohler publishes this CAD for specification and design use. It carries no
open licence, and no grant of redistribution has been identified.** It is
included here for private repair and research on a unit we own.

This repository is public. Before this directory is pushed anywhere public,
somebody needs to make a deliberate call on whether redistributing the
manufacturer's CAD is acceptable, or whether the viewer should instead fetch it
from Kohler's own URL or require the user to drop the file in themselves.

The application does **not** depend on this file being present. The catalog
entry will simply fail to load, and the drag-and-drop loader works regardless —
so removing this directory degrades the demo without breaking the app.

### Not copied

The source directory also holds `.dwg`, `.dxf`, `.skp`, `.rfa` and `.3ds`
versions of the same part, plus four PDF manuals. None are vendored:

- `99693-P.3ds` is the same single mesh with no materials, so it adds nothing
  over the OBJ and would cost another loader on the critical path.
- `.dwg` / `.skp` / `.rfa` have no browser loader and would need offline
  conversion.
- `99693-P.dxf` does carry 3D polymesh data, but three.js ships no DXF loader
  and it is redundant with the OBJ.
- `99693_pfrt.dxf`, `99693_ppln.dxf`, `99693_psde.dxf` are 2D front, plan and
  side elevations (`LINE` and `ARC` entities only). Useful as dimensioned
  reference drawings later; not model geometry.
- The PDFs are documentation, not geometry. The catalog references them by name
  with empty URLs until a hosting decision is made.
