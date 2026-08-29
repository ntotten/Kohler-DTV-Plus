# Parts Viewer

An interactive 3D viewer and STL exporter for service parts, aimed at people
modifying them. Load a manufacturer's published CAD, measure it, and download a
binary STL that a slicer or CAM package will accept.

**This app is deliberately separate from [`../app`](../app).** That one drives a
real shower valve and has to stay lean and predictable; this one is a browsing
and fabrication tool with no hardware surface whatsoever. They share no code, no
build, and no port. Nothing here can move water.

```bash
npm install
npm run dev        # http://localhost:5181
npm run check      # typecheck + unit tests + export gate + build
npm run verify     # export gate on its own, against the vendored CAD
```

## What it does

- **Views** OBJ, STL, 3MF, glTF/GLB, 3DS and PLY — from the catalog or by
  dropping a file on the window.
- **Measures** in export space: envelope, surface area, volume, solid-PLA mass,
  and whether the mesh is actually closed.
- **Reads out coordinates.** Hover the model and the corner shows the point
  under the cursor in millimetres, Z-up — the same frame the exported STL uses,
  so a coordinate can go straight into CAM.
- **Exports binary STL** in millimetres (or inches, for imperial posts), Z-up,
  with the units stated in the filename.

## The thing this app is actually about

Every mesh format in common use records geometry and nothing else. **None of
OBJ, STL, 3MF or PLY records what units the numbers are in, or which way is
up.** A part authored in inches and read as millimetres renders perfectly — it
is simply 25.4× too small, and nothing on screen says so. That error survives
all the way to the machine.

So the catalog requires every file to declare `sourceUnit` and `sourceUpAxis`,
and the loader refuses entries that omit them rather than picking a default. A
guessed unit is worse than a refusal, because it looks like an answer.

The conventions are fixed:

| Space   | Units                | Up axis              | Used for                                          |
| ------- | -------------------- | -------------------- | ------------------------------------------------- |
| Source  | as declared per file | as declared per file | nothing directly                                  |
| Export  | mm                   | Z                    | STL output, all measurements, the pointer readout |
| Display | mm                   | Y                    | the viewport only                                 |

Measurements are taken in **export** space, not display space, so the numbers in
the inspector are the numbers in the downloaded file.

### Why not three's `STLExporter`

`STLExporter` serialises a scene graph, which means you export what you are
_looking at_ — display space, whatever orientation the viewport happens to use.
That is precisely the failure this tool exists to prevent. [`src/core/stl.ts`](src/core/stl.ts)
writes the format directly from the source geometry and the declared units
instead, in about fifty lines, and can be tested in plain Node with no WebGL
context. See its header comment for the full reasoning.

## Verification

`npm run verify` loads every vendored catalog asset, exports it, then
**re-derives the bounding box from the exported bytes** and compares it against
the measured one. It catches a transform applied on one path but not the other,
an asset swapped for a different revision, and any dimension that lands outside
1 mm–2 m (almost always a units error).

Current output:

```
- kohler-dtv-plus/k-99693 (99693-P.obj)
    source        in, z-up
    triangles     4,544
    envelope      133.59 x 30.84 x 84.07 mm
    surface       423.24 cm2
    mesh          OPEN — 224 unshared edges, volume withheld
    read-back     133.59 x 30.84 x 84.07 mm, 0 degenerate facet(s)
    repair        Repaired: welded 11,242 vertices, capped 11 hole(s) with 200
                  triangles, collapsed 2 hairline seam(s). Watertight and manifold.
                  4544 -> 4736 triangles, boundary 222 -> 0, non-manifold 0
    enclosed vol  190.30 cm3
```

The 88 unit tests cover the maths on synthetic geometry; this gate covers the
real manufacturer CAD, which is where the interesting failures actually live.

## Mesh repair

Manufacturer CAD is routinely published as an open surface rather than a solid.
The K-99693 model has 222 boundary edges across 11 open loops and 2 non-manifold
seams. [`src/core/repair.ts`](src/core/repair.ts) closes it, and both meshes are
downloadable — pick "As published" or "Repaired" next to the units selector.

Three distinct fixes, which get conflated and are not the same thing:

| Fix                             | What it addresses                                                                                        | Adds geometry? |
| ------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------- |
| **Welding** (merge by distance) | _Cracks_ — triangles that touch but reference duplicate vertices, so their shared edge is double-counted | no             |
| **Capping** (hole filling)      | _Holes_ — a boundary loop with nothing on the other side                                                 | yes            |
| **Seam collapse**               | _T-junctions_ — a hairline edge claimed by four triangles instead of two                                 | no             |

Welding runs first and is not optional: a hole finder run on unwelded geometry
sees every crack as a hole and would "fill" seams that were never open.

On the K-99693 model: 11,242 vertices welded, 11 loops capped with 200 added
triangles, 2 seams collapsed. 4,544 → 4,736 triangles, watertight and manifold.

**The envelope does not move.** Capping adds interior geometry only; the outer
surface is what a toolpath is cut against, so `npm run verify` fails if any axis
drifts by more than 0.0001 mm. On this part the drift is 0.000000 mm on all three.

### What it refuses to do

A repair that silently invents geometry is worse than no repair, because the
output looks authoritative. So it will not bridge two different loops, guess at
missing internal structure, or re-mesh. A loop that is not planar within
tolerance (default 0.25 mm) is **skipped and reported**, and the result is
declared not watertight rather than flattened into a plausible-looking lie.

## The view cube

A chamfered view cube sits in the corner of the viewport, modelled on Fusion
360's, because that is the one people already know — a gizmo that has to be
learned is worse than no gizmo.

The chamfer is not decoration. A plain cube gives six views; chamfering it
exposes the twelve edges and eight corners as their own faces, so the same
widget also reaches the twelve 45° edge-on views and the eight isometrics:
**26 pick regions in total.**

Around it, in Fusion's arrangement — all of it idling at low opacity and
brightening under the pointer:

| Control            | Where                         | What it does                          |
| ------------------ | ----------------------------- | ------------------------------------- |
| Four triangles     | the four sides                | 90° onto the neighbouring region      |
| A swept arrow band | wrapping the top-right corner | **Roll** ±90° about the line of sight |
| A house            | top left                      | Back to the default framing, refitted |

Roll is the one rotation the cube itself cannot express, because every cube
region implies a canonical up vector. It is also the one that needed care:
OrbitControls derives its orbit frame from `camera.up` **once, in its
constructor**, and caches it in `_quat` / `_quatInverse`. Roll the camera
without refreshing those and the picture turns while dragging still orbits
about the old axis — the view and the mouse quietly stop agreeing, which reads
as broken controls rather than as a stale cache. `rollCamera` refreshes them,
which means reaching past three's public API on purpose; it is guarded, so a
future rename degrades to "rolls but orbits Y-up" instead of throwing.

Each face carries **two lines**: the CAD view name over the export axis it
actually is.

|     | `+`     | `−`      |
| --- | ------- | -------- |
| X   | `RIGHT` | `LEFT`   |
| Y   | `BACK`  | `FRONT`  |
| Z   | `TOP`   | `BOTTOM` |

Both, because each covers the other's failure. The name is what anyone actually
reaches for — nobody thinks "show me −Y". But the name is a convention imposed
on the model rather than a fact about it, and **on this part it is misleading**:
the K-99693's faceplate sits on the CAD's +Y, which the convention calls `BACK`.
The axis line underneath is the one that cannot be wrong, so it stays. Name to
navigate, axis to be sure.

The axes are **export space**, not the viewport's. Every number this app reports
is millimetres Z-up; labelling with the viewport's Y-up axes would be a fourth
convention to hold in your head, and would contradict the readout in the
opposite corner of the same canvas.

Alongside the cube is an **RGB axis triad** — red +X, green +Y, blue +Z, the
convention every CAD and slicer package shares. It anchors at the cube's
`(−X,−Y,−Z)` corner, the one corner all three positive axes lead away from, and
its arms run just outside the three edges meeting there. That is what lets it be
readable in a 132 px widget: the cube's edges are axis-aligned, so an arm along
an export axis is automatically parallel to the edge beside it and can borrow
the edge's length instead of needing a free corner of its own.

The bars are depth-tested but the letters are not, which is deliberate. In a
three-quarter view one axis always points away from you; drawn on top, that bar
lies across the front faces and reads as a rendering bug, so it is occluded.
Its letter is not — otherwise one of three axes goes unnamed, which defeats the
point of labelling them. The letters carry a dark halo to survive landing on a
pale face. Where a receding letter lands is a known rough edge, and no arm length
fixes it — the measurements and the three levers are recorded on `AXIS_ARM` in
[`viewGizmo.ts`](src/scene/viewGizmo.ts).

A triangle rotates the current view 90° and then lands on the nearest of the 26
regions, so a step never leaves the camera at an arbitrary angle even if you had
been orbiting freely.

The cube is **lit and outlined**, and both are needed. Painted flat, its twelve
edge bands and eight corner triangles merge into a blob and you cannot see the
targets you are being asked to click. Lighting gives each facet its own
brightness — the lights hang off the gizmo's own camera, so shading is
view-relative and no side of the cube is ever permanently dark. Thin edges then
separate the facets that happen to catch the light equally. Both are tunable
(`CUBE_LIT`, `CUBE_EDGES`, and the width/colour/opacity beside them); the edges
are `LineSegments2` fat lines because WebGL ignores `linewidth` on ordinary
lines, and a width knob that silently does nothing is worse than none.

### Drag it, don't only click it

The cube can be **dragged to orbit** as well as clicked to snap, which is what
Fusion, Onshape and SolidWorks all do and what anyone who knows one of them will
try in the first minute. Press on any of the 26 regions and nothing happens yet;
travel past a few pixels and it becomes an ordinary orbit, release without
travelling and it snaps exactly as it always did. The slop is 4 px for a mouse
and 10 px for a finger or a stylus, which wander several pixels on their own
while the contact patch shifts.

A cube drag is **geared 2× against a model drag**, and the two should not match.
Dragging the model, the pointer has the whole viewport to travel in and 1:1 is
right — you are pushing the part around and want to stop on a precise angle. The
cube is a 132 px square in the corner; at 1:1 a half-turn means dragging most of
the way across the window, which takes you off the widget entirely and stops it
being something you can flick. At 2× a half-turn is about a third of the canvas
height. The click threshold is in **pixels**, so it is untouched: a click is
still a click at the same travel, only the rotation each pixel buys changes.
Measured, not asserted — a 30 px cube drag lands on pixels identical to a 60 px
model drag.

That also closed an inconsistency that was impossible to explain: dragging the
_empty_ corner beside the cube already orbited, because the hit test returned
nothing there and the event fell through to OrbitControls. One widget, two
behaviours, and no way to tell in advance which you would get.

The chrome stays **click-only**. A cube face is the thing you are turning, so
turning it further is the obvious reading of a drag on it; "roll 90°" is a
discrete command with no continuous form to slide into, and a drag that begins
on a button is ambiguous in a way a drag on the cube is not.

One thing has to be undone for this to work. The `pointerdown` is deliberately
left to reach OrbitControls, so the drag gets the controls' own feel rather than
a second implementation of it — but that means a click that wobbles two pixels
has _already_ queued a rotation, and damping spends only `dampingFactor` of it
per frame. `renderFrame` applies the tween's pose and **then** calls
`controls.update()`, so the leftover would be added on top of every frame of the
animation and the view would settle a degree or two off the axis you clicked,
which is the one thing an axis-aligned view must not be. Deciding "this was a
click" therefore discards the queue first — `stopControlsMomentum` in
[`viewer.ts`](src/scene/viewer.ts), reaching past three's public API under the
same guard as `syncControlsUp`. It is not theoretical: with it disabled, a 2 px
wobble on the `TOP` face lands on a view measurably different from the one the
toolbar's Top button produces.

Picking a view **turns** to it rather than cutting. A hard cut is disorienting —
you have to re-find the part every time — so the camera orbits over ~340 ms with
a mild ease-in and a stronger ease-out, which leaves quickly and lands softly.
The destination is computed by running the real move and rewinding, so the
animation can never drift from where an instant snap would have landed. Speed
and both easing ends are constants in
[`cameraTween.ts`](src/scene/cameraTween.ts); `prefers-reduced-motion` sets the
duration to zero and restores the old instant behaviour.

[`viewGizmo.test.ts`](src/scene/viewGizmo.test.ts),
[`cameraFit.test.ts`](src/scene/cameraFit.test.ts),
[`cameraTween.test.ts`](src/scene/cameraTween.test.ts) and
[`gizmoDrag.test.ts`](src/scene/gizmoDrag.test.ts) cover all of it without a
WebGL context or a DOM — a mis-signed rotation still looks like a rotation, so it
needs a test rather than a look. The tween tests assert the **path**, not just
the endpoints: a plain position lerp arrives in the right place while flying
through the middle of the model on the way. They have already earned it twice: the step
arrows shipped inverted, and roll left a stale component in `camera.up` because
after an orbit that vector leans out of the screen plane and `lookAt` silently
discards the lean.

The roll arrows are **meshes**, not sprites, and that is load-bearing: a sprite
is picked over its whole quad rather than its artwork, so an arrow drawn large
enough to read swallows the clicks meant for the cube faces underneath it — the
cursor says "clickable" and the click does the wrong thing, with nothing in the
rendered image to explain why. A mesh raycasts against its own silhouette, so the
arrows can be any size.

Every layout constant here is a consequence of that constraint plus one bound:
the chamfered cube's 24 vertices are all the same distance from its centre
(`sqrt(CUBE_HALF² + 2·INNER²)`), and an orthographic projection cannot push a
point further out than that — so no orientation can widen the silhouette past it,
and the chrome radii are derived from it rather than eyeballed. The constants
carry that derivation in their comments.

## Decals

Manufacturer CAD is geometry and nothing else — the K-99693 model is one unnamed
group with no materials and no UV coordinates, so there is nothing to texture in
the usual sense. Generating UVs would mean splitting the mesh, which changes the
thing the app exists to preserve.

So artwork goes on as a **decal**: a separate quad floated a fraction of a
millimetre off a measured face, carrying its own image. The source mesh is never
touched, and a decal therefore cannot reach an exported STL — the decal layer is
a sibling of the model in the scene graph, and the exporter reads a geometry
snapshot taken at load. `npm run verify` asserts it rather than trusting it.

Records live in [`src/catalog/decals.json`](src/catalog/decals.json), artwork in
[`public/decals/`](public/decals/), and the maths in
[`src/core/decals.ts`](src/core/decals.ts), which has no renderer dependency.

### Orientation is not in the mesh

The K-99693 is a **portrait** device — the K-99694 bracket drawing gives it as
84 mm wide by 143 mm tall with the wiring boss at the bottom — but the CAD is
authored on its side: the product's vertical runs along the model's **X** axis,
product-down at +X. The faceplate quad measures 131.07 × 81.47 mm in CAD terms
and 81.47 × 131.07 mm as a person sees it on the wall.

Nothing in the mesh says so. The face is a blank symmetric rectangle, so which
end is up came from the bracket drawing, and it is recorded as such in the decal
record's provenance note rather than presented as a measurement.

The practical consequence: the standard views are CAD views and none of them
shows the interface upright. **"Look at it, upright"** in the Appearance panel
does, and it needs no per-part configuration — the anchor's `v` vector _is_ the
artwork's up direction, so any decal on any part can be viewed the right way up.

### An anchor is three vectors

Anchors are written in **export space — millimetres, Z-up**, the same frame the
pointer readout prints. Authoring a decal is a hover-and-type job: point at the
corners of the face, read the millimetres off the corner readout, type them in.

```jsonc
"anchor": {
  "origin": [65.5418, 15.2734, 40.7279],  // image (0,0): bottom-left to a viewer in front
  "u":      [0, 0, -81.4677],             // image +X edge. ITS LENGTH IS THE WIDTH.
  "v":      [-131.0691, 0, 0]             // image +Y edge. Its length is the height.
}
```

Position, size, orientation and handedness all fall out of those three, so there
is no separate rotate/flip/scale field to get backwards. The outward normal is
`u × v`, which makes handedness **self-checking**: swap the two edges and the
decal faces into the part, which the verify gate catches by name.

`v` runs along −X here because that is the product's up; `u` then falls out as
−Z, because a viewer standing in front looks along −Y and, with −X overhead,
their right hand is on −Z. That is what makes `u × v` come out as +Y.

### What the gate checks

`npm run verify` refuses a decal that is wrong in a way rendering would not
reveal:

| Check                                   | Catches                                                                 |
| --------------------------------------- | ----------------------------------------------------------------------- |
| `u · v ≈ 0`                             | A sheared anchor — always a mistyped corner                             |
| Artwork aspect vs face aspect, 1%       | Silently squashed artwork, unless `"fit": "contain"` says so on purpose |
| Every corner within 0.05 mm of the mesh | An anchor that is internally consistent and simply not on the part      |
| Decal normal vs the facet beneath it    | `u` and `v` swapped, which buries the artwork inside the part           |
| Export triangle count unchanged         | A decal reaching the downloaded STL                                     |

The aspect check is the one that earns its keep. A stretched screenshot is the
same class of error as a guessed unit: it renders perfectly, and it is wrong.

### Vector artwork

SVG is the preferred format — it is text, so humans and agents can edit it
together, it diffs, and it rasterises at whatever resolution the face needs. The
K-99693 decal uses a `viewBox` of exactly **ten units per millimetre of the real
part**, so a rectangle 200 units wide is 20 mm wide on the faceplate and editing
it is dimensioned work.

Two constraints, both learned the hard way: an SVG rasterised through an `<img>`
is a sandboxed XML document, so it **cannot fetch webfonts or linked images**,
and its comments **may not contain a double hyphen** — one occurrence makes the
whole file fail to parse with nothing on screen to say why.

### Toward Maker Galaxy

`core/decals.ts` is deliberately renderer-agnostic: it takes a record and returns
four corners, four UVs and a normal. The record shape follows Maker Galaxy
Studio's markup model — `sourceModelId`, a geometry anchor, a style, a
provenance note — so a decal set moves across as a project-linked review record
rather than needing a translation layer, and its Studio viewer can feed the
corners into its own scene graph without adopting anything else from here.

The transferable idea is smaller than the code: **enrichment is a sidecar, not
an edit.** Geometry arrives from a manufacturer or a generator and should stay
byte-comparable to what it arrived as; everything added for presentation lives
beside it, in a declared coordinate frame, with a gate that checks the two still
agree.

## Known limitations

1. **The repaired K-99693 is a closed shell, not a solid model of the part.**
   Its enclosed volume (190.30 cm³) counts the hollow interior, so the "solid
   PLA" mass figure is an upper bound on a part that isn't solid.
2. **The CAD contains no internal structure at all** — no PCB, no connector, no
   ribs or bosses. It is authority for the outside of the part and for nothing
   behind it. Establish internal clearances from the physical part.
3. **Parts are single meshes.** The Kohler CAD is one unnamed group, so buttons
   and bezel cannot be picked or isolated separately without splitting the mesh
   by hand in Blender first.
4. **Dropped files assume mm and Z-up**, because nothing in the file says
   otherwise. The status line says so every time. Use a catalog entry for
   anything headed to a machine.
5. **DWG, SKP and RFA are not supported** and will not be — they have no browser
   loader and need offline conversion. DXF is not supported either; three.js
   ships no loader for it.

## Adding a part

Add an entry to [`src/catalog/catalog.json`](src/catalog/catalog.json). The
validator will reject it unless every file declares `sourceUnit` and
`sourceUpAxis`.

Establish those from a dimensioned source — a spec sheet, a drawing, or a
measurement of the physical part — and record how in `provenanceNote`. The
K-99693 entry is the worked example: its bounding box was checked against the
published spec sheet on all three axes before the units were declared.

Vendored assets go in `public/models/<familyId>/` and must be recorded in
[`public/models/PROVENANCE.md`](public/models/PROVENANCE.md).

## Relationship to Maker Galaxy

The catalog's `files[]` entries deliberately use the same `id` / `name` /
`format` / `url` / `isDefaultViewer` fields as Maker Galaxy's
`src/maker-galaxy/data/models.json`, and [`src/scene/cameraFit.ts`](src/scene/cameraFit.ts)
is a behaviour-preserving port of its `viewerHelpers.js`. A part described here
should move into that catalog without a translation layer.

Two fields are additions Maker Galaxy does not yet have: `sourceUnit` and
`sourceUpAxis`. Its current viewer assumes STL and 3MF are already in
millimetres, which holds for maker-authored models and does not hold for
manufacturer CAD.
