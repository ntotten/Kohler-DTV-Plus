import * as THREE from 'three';
import { LineSegments2 } from 'three/addons/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/addons/lines/LineSegmentsGeometry.js';
import { LineMaterial } from 'three/addons/lines/LineMaterial.js';

// The orientation gizmo: a chamfered view cube in the corner of the viewport,
// with curved arrows for 90-degree steps. Modelled on Fusion 360's, because
// that is the one people already know — a gizmo that has to be learned is worse
// than no gizmo.
//
// TWENTY-SIX PICK REGIONS
//
// The chamfer is not decoration. A plain cube gives six views; chamfering it
// exposes the twelve edges and eight corners as separate faces, so the same
// widget also reaches the twelve 45-degree edge-on views and the eight
// isometrics. That is the whole reason Fusion's cube is chamfered, and it is
// most of the value of the control.
//
//   6 faces   -> the orthographic views
//   12 edges  -> 45 degrees between two faces
//   8 corners -> the isometrics
//
// LABELLED IN EXPORT SPACE, ON TWO LINES
//
// Each face carries the view name people expect — FRONT / BACK / LEFT / RIGHT /
// TOP / BOTTOM — over the export axis it actually is:
//
//        RIGHT              the name:  what a CAD user reaches for
//         +X                the axis:  what is true of any model
//
// The axis line is the one that cannot lie, and it is why it stays. The
// viewport is Y-up because three is Y-up, while every number this app reports —
// the pointer readout, the measurements, the decal anchors, the exported STL —
// is millimetres Z-up. Labelling with the viewport's own axes would be a fourth
// convention for the user to hold, and would contradict the readout in the
// opposite corner of the same canvas.
//
// The names are a CAD convention imposed on the model, and on THIS part the
// convention is misleading: the K-99693's faceplate sits on the CAD's +Y, which
// the convention calls BACK. That is not a bug in the mapping — it is what the
// mapping honestly reports, and it is exactly why the axis line sits under the
// name rather than replacing it. Read the name to navigate; read the axis to be
// sure.
//
//   export +X  ->  display +X       RIGHT
//   export +Y  ->  display -Z       BACK    (the K-99693's faceplate)
//   export +Z  ->  display +Y       TOP
//
// Rendered as a second viewport in the same canvas rather than a second WebGL
// context: one renderer, one animation loop, no chance of the two drifting out
// of sync by a frame.

/**
 * Pixels along each edge of the gizmo's viewport — the ONLY layout number in
 * pixels, and so the only one to touch to resize the whole widget.
 *
 * Everything else below is in gizmo units against `EXTENT`, so the arrangement
 * scales as one piece and every proportion inside it is fixed by construction.
 * Was 176; 132 is that at 75%, because the widget was crowding the canvas.
 */
const SIZE = 132;
/**
 * Gap from the canvas corner. Deliberately NOT scaled with `SIZE`: it matches
 * the readout's inset in the opposite corner, and the two must stay flush.
 */
const INSET = 12;
/** Orthographic half-extent. The cube is 1 unit half-width; the chrome sits outside it. */
const EXTENT = 2.14;

const CUBE_HALF = 1;
/**
 * How far the chamfer cuts in. 0 is a plain cube; 1 is an octahedron.
 *
 * This is the edge/corner targets' size dial, and they are the fiddly ones to
 * hit: an edge chamfer is CHAMFER·sqrt(2) wide and a corner is a triangle of
 * that side. Raised from 0.28 to make them comfortably clickable — the corners
 * gain about 84% in area — at the cost of face area for the labels, which is
 * the only thing pushing back.
 */
const CHAMFER = 0.38;
const INNER = CUBE_HALF - CHAMFER;

/** Step triangles, on the four sides. */
const STEP_RADIUS = 1.58;
const STEP_SCALE = 0.44;
/**
 * Roll arrows: one concentric band wrapping the cube's top-right corner, in the
 * gap between the top and the right step triangle.
 *
 * `ROLL_INNER` sits just outside the cube's silhouette in EVERY orientation,
 * which is a bound rather than a guess: the chamfered cube's 24 vertices are
 * the permutations of (±CUBE_HALF, ±INNER, ±INNER), so all of them are the same
 * distance from the centre — R_v = sqrt(CUBE_HALF² + 2·INNER²) — and a
 * projection can never push one further out than that. Coming closer would lay
 * the band over the cube, and the chrome wins every contested pixel.
 *
 * R_v is 1.330 at the current CHAMFER, leaving 0.233 of clearance. Raising
 * CHAMFER lowers R_v, so bigger chamfers only ever make this safer; CUTTING it
 * is what would need these radii revisited.
 */
const ROLL_INNER = 1.563;
const ROLL_OUTER = 1.857;
/**
 * The head's barbs, wider than the band on both sides — and deliberately NOT
 * derived from it, so thinning the sweep leaves the heads alone.
 */
const ROLL_HEAD_INNER = 1.38;
const ROLL_HEAD_OUTER = 2.04;
/** How much of each arrow's sweep the head takes. */
const ROLL_HEAD_SWEEP = THREE.MathUtils.degToRad(15);
/**
 * Angular limits, set by the step sprites either side of the gap. A sprite is
 * picked over its whole QUAD, not its artwork, and the top step's quad spans
 * x within ±STEP_SCALE/2 — so what actually has to clear is not an angle but
 * that band: the arrow's tip, its closest approach, sits at x = 0.297 against
 * the quad's 0.22. Stated as an angle it looks like 0.8 degrees of margin,
 * which badly understates it. The two arrows meet either side of the diagonal.
 */
const ROLL_MAX = THREE.MathUtils.degToRad(80);
const ROLL_MIN = THREE.MathUtils.degToRad(10);
const ROLL_MEET = THREE.MathUtils.degToRad(45);
const ROLL_SPLIT = THREE.MathUtils.degToRad(2.5);
/**
 * Home, opposite the roll pair and tucked in as close as its QUAD allows.
 *
 * It is a sprite, so it is picked over its whole square, not over the house
 * drawn inside it. Sitting on the diagonal, the square's nearest corner is the
 * binding constraint: it reaches the cube once `(|x| - HOME_SCALE / 2) *
 * sqrt(2)` drops to R_v above. The offset chosen here puts that corner at
 * exactly `ROLL_INNER`, so the house and the roll band keep the same clearance.
 *
 * That the offset reads 1.43 and the OLD R_v was also 1.43 is a coincidence of
 * two unrelated quantities; they are not connected, and R_v is now 1.330.
 */
const HOME_POSITION: [number, number] = [-1.43, 1.43];
const HOME_SCALE = 0.65;

// ---------------------------------------------------------------- axis triad
//
// An RGB triad in the Fusion/Cura idiom: red +X, green +Y, blue +Z, in EXPORT
// space, so it agrees with the face labels and with every number this app
// reports.
//
// WHY IT CAN BE THIS BIG IN A 132px WIDGET
//
// The cube's twelve edges are all axis-aligned, so an arm drawn along an export
// axis is automatically PARALLEL to the three cube edges nearest it. That means
// the triad does not need a free corner of its own to live in — it can run the
// length of an edge, just outside the cube's skin. An arm is ~1.2 units, about
// 37px, rather than the ~20px a corner-boxed triad would have had.
//
// It is anchored at the export (−X, −Y, −Z) corner, because that is the one
// corner all three POSITIVE axes lead away from. Everything below is a knob;
// the geometry is rebuilt from these values alone.

const AXIS_COLORS: Record<(typeof AXES)[number], number> = {
  X: 0xff5a5a,
  Y: 0x54d97a,
  Z: 0x5a9dff,
};

/**
 * How far the triad stands off the cube, applied on BOTH axes perpendicular to
 * each arm — so an arm sits diagonally outside the edge it parallels rather
 * than resting on it. This is the "small gap": coplanar geometry z-fights, and
 * a gap is the fix that does not require depth tricks.
 */
const AXIS_GAP = 0.1;
/**
 * Arm length along the edge. The edge itself is 2·CUBE_HALF, so this is ~60%.
 *
 * 1.2 is a compromise with a KNOWN, measured limitation, recorded here so the
 * next person does not re-derive it:
 *
 * In a three-quarter view the receding arm's letter lands somewhere on the cube
 * face, and no arm length avoids that — it only slides the letter around. Its
 * projected position tracks a straight screen line out from the anchor, so:
 *
 *   1.2  letter sits by the face's axis text, ~0.13 units off it
 *   1.5  letter clears that but reaches the face NAME instead, and the X and Z
 *        letters push out far enough to foul the step triangles
 *   ~0.4 letter finally clears the cube silhouette — but the arm is 12px and
 *        no longer reads as running along an edge at all
 *
 * The dark halo on the letters is what makes 1.2 acceptable rather than the
 * geometry. Fixing it properly needs the labels pushed radially outward in
 * SCREEN space per frame, which then has to be reconciled with the chrome that
 * already occupies radius 1.36–1.86. Not attempted.
 */
const AXIS_ARM = 1.2;
/** Arm cross-section. Square bars, not lines: WebGL ignores `linewidth`. */
const AXIS_THICKNESS = 0.052;
/** Gap between an arm's tip and the centre of its letter. */
const AXIS_LABEL_OFFSET = 0.2;
const AXIS_LABEL_SCALE = 0.42;
/**
 * Whether the cube occludes the triad. TRUE, and that is worth explaining,
 * because "always on top" is the tempting default and it looks wrong here.
 *
 * In any three-quarter view one axis necessarily points AWAY from the viewer.
 * Drawn on top, that arm lies across the front faces and its letter collides
 * with the face labels — it reads as a rendering bug rather than as depth.
 * Depth-tested, it simply disappears behind the cube, which is what the eye
 * expects and is the same thing every CAD origin triad does.
 *
 * The anchor is far enough out (it projects ~1.80 from centre, against the
 * cube's 1.33 silhouette) that the corner and the near part of every arm stay
 * visible regardless of orientation. The one exception is looking straight down
 * the anchor's own diagonal — the export +X/+Y/+Z isometric — where the triad
 * hides behind the cube entirely. One octant of eight.
 *
 * Set false to overlay it instead.
 */
const AXIS_DEPTH_TEST = true;
/**
 * The LETTERS, separately, are NOT depth-tested — and the split is the point.
 *
 * Depth-testing the bars is what makes the receding axis read as depth. But it
 * also buries that axis's letter behind the cube, so in any three-quarter view
 * one of the three axes goes unnamed, which defeats a labelled triad. A whole
 * bar lying across the cube reads as a bug; a single letter over it reads as a
 * label, so the letter is the part worth floating.
 *
 * Letters carry their own dark halo (see `axisLetterTexture`) to stay legible
 * against the pale cube faces they sometimes land on.
 */
const AXIS_LABEL_DEPTH_TEST = false;

const CHROME_IDLE_OPACITY = 0.38;
const CHROME_HOVER_OPACITY = 1;

// ------------------------------------------------- cube shading and edges
//
// A flat-shaded chamfered cube is nearly unreadable: twelve edge bands and
// eight corner triangles all painted one colour merge into a single blob, and
// the user cannot see the targets they are being asked to click. Two cues fix
// it, and they are independent so the balance can be tuned:
//
//   LIGHTING  gives every facet a different brightness, because every facet has
//             a different normal. This is the cue that does the real work.
//   EDGES     draw the boundaries explicitly. Cheap, crisp, and unlike lighting
//             it still separates two facets that happen to catch the light
//             equally.

/** Shade the cube with lights instead of painting it flat. */
const CUBE_LIT = true;
/**
 * Lights are parented to the GIZMO CAMERA, not the scene, so the shading is
 * view-relative: a facet's brightness depends on how it is turned towards the
 * viewer, which is exactly the cue that separates neighbouring chamfers. Fixed
 * scene lights would instead leave whole sides of the cube permanently dark.
 */
const CUBE_AMBIENT_INTENSITY = 1.55;
const CUBE_KEY_INTENSITY = 1.5;
/** Key light position in CAMERA-local space: up, left and behind the viewer. */
const CUBE_KEY_DIRECTION = new THREE.Vector3(-0.4, 0.75, 1);

/** Outline every facet. Independent of `CUBE_LIT` — use either, or both. */
const CUBE_EDGES = true;
/**
 * Edge width in PIXELS. Real pixels: these are `LineSegments2` fat lines, not
 * `THREE.Line`, because WebGL ignores `linewidth` on the latter and it would
 * silently stay 1px however this was set. That silent failure is the whole
 * reason for the extra dependency.
 */
const CUBE_EDGE_WIDTH = 1.3;
const CUBE_EDGE_COLOR = 0x0d1220;
const CUBE_EDGE_OPACITY = 0.5;

const FACE_COLOR = 0x8b98b5;
const CHAMFER_COLOR = 0x5c688a;
const HOVER_COLOR = 0xffb347;
const ARROW_COLOR = 0xc7d0e4;
const LABEL_DARK = '#0b0d16';
/**
 * The axis line. Same weight and near-black as the name — the hierarchy is
 * carried by SIZE and a touch of contrast, not by a lighter weight, because at
 * a 39px face a light weight just reads as blur.
 */
const LABEL_AXIS = 'rgba(11, 13, 22, 0.72)';

// Face type. All three are fractions of the face texture's edge. Sized for
// legibility at a ~39px face rather than for comfortable margins: the type is
// meant to run close to the border.
const FACE_FONT_WEIGHT = 700;
const FACE_NAME_SIZE = 0.32;
const FACE_AXIS_SIZE = 0.27;
/** Widest the type may run, as a fraction of the face. The border sits at 0.95. */
const FACE_TEXT_WIDTH = 0.88;

const AXES = ['X', 'Y', 'Z'] as const;

/** Export axis -> display direction. The one place the mapping is written. */
const EXPORT_TO_DISPLAY: Record<(typeof AXES)[number], THREE.Vector3> = {
  X: new THREE.Vector3(1, 0, 0),
  Y: new THREE.Vector3(0, 0, -1),
  Z: new THREE.Vector3(0, 1, 0),
};

/**
 * Export axis -> the CAD view name for each end of it.
 *
 * The standard Z-up convention: the FRONT view looks along +Y, so the face you
 * are looking AT is the −Y one. Kept as a table rather than computed, because
 * it is a convention and conventions should be legible, not derived.
 */
const FACE_NAMES: Record<(typeof AXES)[number], { positive: string; negative: string }> = {
  X: { positive: 'RIGHT', negative: 'LEFT' },
  Y: { positive: 'BACK', negative: 'FRONT' },
  Z: { positive: 'TOP', negative: 'BOTTOM' },
};

export type GizmoStep = 'left' | 'right' | 'up' | 'down';

export type GizmoPick =
  /** A cube region: look at the model from this display-space direction. */
  | { kind: 'view'; towards: THREE.Vector3; label: string }
  /** A side triangle: turn 90 degrees this way and land on the nearest region. */
  | { kind: 'step'; step: GizmoStep; label: string }
  /** A curved arrow: roll about the line of sight. Sign is the on-screen turn. */
  | { kind: 'roll'; radians: number; label: string }
  /** The house: back to the default three-quarter view, refitted. */
  | { kind: 'home'; label: string };

interface Region {
  object: THREE.Object3D;
  pick: GizmoPick;
  /** Restored when the pointer leaves. */
  restore: () => void;
  highlight: () => void;
}

export interface ViewGizmoHandles {
  /** Draw into the corner of the canvas. Call after the main scene is rendered. */
  render(renderer: THREE.WebGLRenderer, mainCamera: THREE.PerspectiveCamera): void;
  /** Update the hover highlight and report what is under the pointer. */
  hover(clientX: number, clientY: number, renderer: THREE.WebGLRenderer): GizmoPick | null;
  /**
   * Put out the highlight without a pointer position. Used while the cube is
   * being dragged to orbit, where the pointer is over a region but nothing is
   * being offered as a target.
   */
  clearHover(): void;
  /** What is under the pointer, without changing the highlight. */
  hit(clientX: number, clientY: number, renderer: THREE.WebGLRenderer): GizmoPick | null;
  dispose(): void;
}

export function createViewGizmo(): ViewGizmoHandles {
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-EXTENT, EXTENT, EXTENT, -EXTENT, 0.1, 100);
  // The arrows hang off the camera so they stay put on screen while the cube
  // turns underneath them, which is how Fusion's behave.
  scene.add(camera);

  const regions: Region[] = [];
  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let hovered: Region | null = null;

  if (CUBE_LIT) {
    scene.add(new THREE.AmbientLight(0xffffff, CUBE_AMBIENT_INTENSITY));
    const key = new THREE.DirectionalLight(0xffffff, CUBE_KEY_INTENSITY);
    key.position.copy(CUBE_KEY_DIRECTION);
    // Parented to the camera so the shading is view-relative — see the note on
    // CUBE_KEY_DIRECTION. A DirectionalLight aims at its `target`, which
    // defaults to the origin of ITS OWN parent, so the target rides along too.
    camera.add(key);
    camera.add(key.target);
  }

  buildCube(scene, regions);
  buildAxisTriad(scene);
  buildChrome(camera, regions);

  function viewportOf(renderer: THREE.WebGLRenderer): { x: number; y: number; size: number } {
    // CSS pixels, NOT device pixels: setViewport and setScissor apply the
    // renderer's pixel ratio themselves. Applying it here as well puts the
    // gizmo off the canvas on any HiDPI display.
    const target = renderer.getSize(new THREE.Vector2());
    return { x: target.x - SIZE - INSET, y: INSET, size: SIZE };
  }

  function render(renderer: THREE.WebGLRenderer, mainCamera: THREE.PerspectiveCamera): void {
    // Match the main camera's orientation at a fixed distance, so the cube
    // turns with the part but never changes size.
    const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(mainCamera.quaternion);
    camera.position.copy(forward).multiplyScalar(-4);
    camera.up.copy(mainCamera.up);
    camera.lookAt(0, 0, 0);

    const view = viewportOf(renderer);
    const previousAutoClear = renderer.autoClear;
    renderer.autoClear = false;
    renderer.setScissorTest(true);
    renderer.setViewport(view.x, view.y, view.size, view.size);
    renderer.setScissor(view.x, view.y, view.size, view.size);
    renderer.clearDepth();
    renderer.render(scene, camera);
    renderer.setScissorTest(false);

    // Hand the full canvas back, or the next main render draws into the corner.
    const target = renderer.getSize(new THREE.Vector2());
    renderer.setViewport(0, 0, target.x, target.y);
    renderer.autoClear = previousAutoClear;
  }

  function pick(clientX: number, clientY: number, renderer: THREE.WebGLRenderer): Region | null {
    const rect = renderer.domElement.getBoundingClientRect();
    const x = clientX - rect.left;
    // CSS pixels from the canvas's bottom-left, matching the viewport origin.
    const y = rect.height - (clientY - rect.top);
    const left = rect.width - SIZE - INSET;
    if (x < left || x > left + SIZE || y < INSET || y > INSET + SIZE) return null;

    pointer.x = ((x - left) / SIZE) * 2 - 1;
    pointer.y = ((y - INSET) / SIZE) * 2 - 1;
    // The arrows are camera children, so their world matrices are only correct
    // once the camera's is.
    scene.updateMatrixWorld(true);
    raycaster.setFromCamera(pointer, camera);
    const hit = raycaster.intersectObjects(
      regions.map((r) => r.object),
      false,
    )[0];
    return hit ? (regions.find((r) => r.object === hit.object) ?? null) : null;
  }

  function setHovered(region: Region | null): void {
    if (hovered === region) return;
    hovered?.restore();
    hovered = region;
    hovered?.highlight();
  }

  return {
    render,
    hover: (clientX, clientY, renderer) => {
      const region = pick(clientX, clientY, renderer);
      setHovered(region);
      return region?.pick ?? null;
    },
    clearHover: () => setHovered(null),
    hit: (clientX, clientY, renderer) => pick(clientX, clientY, renderer)?.pick ?? null,
    dispose: () => {
      scene.traverse((object) => {
        const mesh = object as THREE.Mesh;
        const sprite = object as THREE.Sprite;
        if (mesh.isMesh) mesh.geometry.dispose();
        if (!mesh.isMesh && !sprite.isSprite) return;
        const material = (mesh.isMesh ? mesh.material : sprite.material) as THREE.SpriteMaterial;
        material.map?.dispose();
        material.dispose();
      });
    },
  };
}

/**
 * Turn a 90-degree step into the direction to look from next.
 *
 * The step rotates the current view direction about the camera's own up or
 * right vector, then lands on the nearest of the cube's 26 regions. Snapping to
 * a region rather than applying the rotation directly means the arrows always
 * arrive somewhere the cube can also reach, even from a freely orbited angle.
 */
export function stepDirection(
  step: GizmoStep,
  camera: THREE.PerspectiveCamera,
  target: THREE.Vector3,
): THREE.Vector3 {
  const view = camera.position.clone().sub(target).normalize();
  const up = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 1).normalize();
  const right = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 0).normalize();

  // Each arrow brings the region it points at round to the front, which is the
  // convention every view cube uses: the arrow on the right of the cube shows
  // you what is currently on the right.
  const quarter = Math.PI / 2;
  const rotated = view.clone();
  if (step === 'left') rotated.applyAxisAngle(up, -quarter);
  if (step === 'right') rotated.applyAxisAngle(up, quarter);
  if (step === 'up') rotated.applyAxisAngle(right, -quarter);
  if (step === 'down') rotated.applyAxisAngle(right, quarter);

  return nearestRegionDirection(rotated);
}

/** The closest of the cube's 26 outward directions. */
export function nearestRegionDirection(to: THREE.Vector3): THREE.Vector3 {
  let best = new THREE.Vector3(0, 0, 1);
  let bestDot = -Infinity;
  for (let x = -1; x <= 1; x++) {
    for (let y = -1; y <= 1; y++) {
      for (let z = -1; z <= 1; z++) {
        if (!x && !y && !z) continue;
        const candidate = new THREE.Vector3(x, y, z).normalize();
        const dot = candidate.dot(to);
        if (dot > bestDot) {
          bestDot = dot;
          best = candidate;
        }
      }
    }
  }
  return best;
}

// ---------------------------------------------------------------- cube

function buildCube(scene: THREE.Scene, regions: Region[]): void {
  const e = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)];

  // Every facet's boundary, gathered as it is built and turned into one line
  // object at the end. Collected here rather than derived afterwards with
  // EdgesGeometry, because the facets are already exact polygons — re-deriving
  // them from a merged mesh would mean guessing a crease angle.
  const outlines: THREE.Vector3[][] = [];

  // Faces. Labelled with the EXPORT axis whose display direction they face.
  for (const axis of AXES) {
    for (const sign of [1, -1] as const) {
      const normal = EXPORT_TO_DISPLAY[axis].clone().multiplyScalar(sign);
      const name = sign > 0 ? FACE_NAMES[axis].positive : FACE_NAMES[axis].negative;
      const signed = `${sign > 0 ? '+' : '−'}${axis}`;
      const label = `${name} (${signed})`;
      const { right, up } = faceBasis(normal);
      const centre = normal.clone().multiplyScalar(CUBE_HALF);
      const corners = [
        centre.clone().addScaledVector(right, -INNER).addScaledVector(up, -INNER),
        centre.clone().addScaledVector(right, INNER).addScaledVector(up, -INNER),
        centre.clone().addScaledVector(right, INNER).addScaledVector(up, INNER),
        centre.clone().addScaledVector(right, -INNER).addScaledVector(up, INNER),
      ];
      const mesh = new THREE.Mesh(
        polygon(corners, normal, true),
        cubeMaterial(FACE_COLOR, faceTexture(name, signed)),
      );
      outlines.push(corners);
      add(scene, regions, mesh, { kind: 'view', towards: normal, label }, FACE_COLOR);
    }
  }

  // Edge chamfers: a quad bridging the trimmed borders of two faces.
  for (let i = 0; i < 3; i++) {
    for (let j = i + 1; j < 3; j++) {
      const k = 3 - i - j;
      for (const si of [1, -1] as const) {
        for (const sj of [1, -1] as const) {
          const corners = [
            span(e, i, si * CUBE_HALF, j, sj * INNER, k, -INNER),
            span(e, i, si * INNER, j, sj * CUBE_HALF, k, -INNER),
            span(e, i, si * INNER, j, sj * CUBE_HALF, k, INNER),
            span(e, i, si * CUBE_HALF, j, sj * INNER, k, INNER),
          ];
          const normal = e[i].clone().multiplyScalar(si).addScaledVector(e[j], sj).normalize();
          const mesh = new THREE.Mesh(polygon(corners, normal, false), cubeMaterial(CHAMFER_COLOR));
          outlines.push(corners);
          add(
            scene,
            regions,
            mesh,
            { kind: 'view', towards: normal, label: 'edge' },
            CHAMFER_COLOR,
          );
        }
      }
    }
  }

  // Corner chamfers: a triangle across the three trimmed face borders.
  for (const sx of [1, -1] as const) {
    for (const sy of [1, -1] as const) {
      for (const sz of [1, -1] as const) {
        const corners = [
          new THREE.Vector3(sx * CUBE_HALF, sy * INNER, sz * INNER),
          new THREE.Vector3(sx * INNER, sy * CUBE_HALF, sz * INNER),
          new THREE.Vector3(sx * INNER, sy * INNER, sz * CUBE_HALF),
        ];
        const normal = new THREE.Vector3(sx, sy, sz).normalize();
        const mesh = new THREE.Mesh(polygon(corners, normal, false), cubeMaterial(CHAMFER_COLOR));
        outlines.push(corners);
        add(
          scene,
          regions,
          mesh,
          { kind: 'view', towards: normal, label: 'corner' },
          CHAMFER_COLOR,
        );
      }
    }
  }

  if (CUBE_EDGES) scene.add(buildEdges(outlines));
}

/**
 * One cube facet's material.
 *
 * Lambert when lit: the cube wants flat, predictable shading that separates
 * facets, not highlights. A specular model would put a moving hotspot on a
 * 40px widget, which is noise rather than information.
 *
 * `polygonOffset` pushes the filled facets a hair away from the viewer so the
 * edge lines, which are exactly coplanar with them, win the depth test instead
 * of z-fighting along their whole length.
 */
function cubeMaterial(color: number, map?: THREE.Texture): THREE.Material {
  const settings = {
    color,
    map,
    polygonOffset: CUBE_EDGES,
    polygonOffsetFactor: 1,
    polygonOffsetUnits: 1,
  };
  return CUBE_LIT ? new THREE.MeshLambertMaterial(settings) : new THREE.MeshBasicMaterial(settings);
}

/**
 * All facet boundaries as one fat-line object.
 *
 * Every cube edge borders two facets, so each segment is offered twice; the
 * duplicates are dropped. Not for performance — drawing a translucent line
 * twice over itself doubles its opacity, and the shared edges would come out
 * visibly darker than the rest.
 */
function buildEdges(outlines: THREE.Vector3[][]): LineSegments2 {
  const key = (v: THREE.Vector3): string => `${v.x.toFixed(4)},${v.y.toFixed(4)},${v.z.toFixed(4)}`;
  const seen = new Set<string>();
  const positions: number[] = [];

  for (const corners of outlines) {
    for (let i = 0; i < corners.length; i++) {
      const a = corners[i];
      const b = corners[(i + 1) % corners.length];
      // Order-independent, so A->B and B->A collide as they should.
      const id = [key(a), key(b)].sort().join('|');
      if (seen.has(id)) continue;
      seen.add(id);
      positions.push(a.x, a.y, a.z, b.x, b.y, b.z);
    }
  }

  const geometry = new LineSegmentsGeometry();
  geometry.setPositions(positions);
  const material = new LineMaterial({
    color: CUBE_EDGE_COLOR,
    linewidth: CUBE_EDGE_WIDTH,
    transparent: true,
    opacity: CUBE_EDGE_OPACITY,
    // Fat lines are screen-space: the shader needs the viewport it is being
    // drawn into to turn `linewidth` into pixels. That is the GIZMO's square,
    // not the canvas — passing the canvas size would scale the edges with the
    // window.
    resolution: new THREE.Vector2(SIZE, SIZE),
  });
  return new LineSegments2(geometry, material);
}

function add(
  scene: THREE.Object3D,
  regions: Region[],
  mesh: THREE.Mesh,
  pick: GizmoPick,
  baseColor: number,
): void {
  scene.add(mesh);
  // Basic or Lambert depending on CUBE_LIT; both carry `color`, which is all
  // the hover swap needs.
  const material = mesh.material as THREE.MeshBasicMaterial | THREE.MeshLambertMaterial;
  regions.push({
    object: mesh,
    pick,
    highlight: () => material.color.setHex(HOVER_COLOR),
    restore: () => material.color.setHex(baseColor),
  });
}

/** A point at `a` along axis `i`, `b` along `j` and `c` along `k`. */
function span(
  e: THREE.Vector3[],
  i: number,
  a: number,
  j: number,
  b: number,
  k: number,
  c: number,
): THREE.Vector3 {
  return e[i].clone().multiplyScalar(a).addScaledVector(e[j], b).addScaledVector(e[k], c);
}

/**
 * In-plane basis for a face, chosen so its label reads the right way up when
 * the camera is looking straight at it.
 */
function faceBasis(normal: THREE.Vector3): { right: THREE.Vector3; up: THREE.Vector3 } {
  // Any reference up parallel to the normal degenerates, so the two horizontal
  // faces borrow a different one — the same trick `snapToDirection` uses.
  const reference =
    Math.abs(normal.y) > 0.9
      ? new THREE.Vector3(0, 0, normal.y > 0 ? -1 : 1)
      : new THREE.Vector3(0, 1, 0);
  const right = new THREE.Vector3().crossVectors(reference, normal).normalize();
  const up = new THREE.Vector3().crossVectors(normal, right).normalize();
  return { right, up };
}

/**
 * A triangle or quad from ordered corners, wound so it faces `normal`.
 *
 * The winding is corrected rather than assumed: the edge and corner chamfers
 * are generated from sign loops, and half of them come out back-facing. A
 * back-facing pick region is invisible and unclickable, which is a tedious bug
 * to chase for the sake of a cross product.
 */
function polygon(
  corners: THREE.Vector3[],
  normal: THREE.Vector3,
  withUv: boolean,
): THREE.BufferGeometry {
  const facing = new THREE.Vector3()
    .subVectors(corners[1], corners[0])
    .cross(new THREE.Vector3().subVectors(corners[2], corners[0]));
  const ordered = facing.dot(normal) >= 0 ? corners : [...corners].reverse();

  const geometry = new THREE.BufferGeometry();
  const positions = new Float32Array(ordered.length * 3);
  const normals = new Float32Array(ordered.length * 3);
  ordered.forEach((corner, index) => {
    positions.set([corner.x, corner.y, corner.z], index * 3);
    normals.set([normal.x, normal.y, normal.z], index * 3);
  });
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));

  if (withUv) {
    // Quads only, and only faces carry a label. Reversal above would put the
    // UVs on backwards, so they follow the same order.
    const uv = [
      [0, 0],
      [1, 0],
      [1, 1],
      [0, 1],
    ];
    const source = facing.dot(normal) >= 0 ? uv : [...uv].reverse();
    geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(source.flat()), 2));
  }

  geometry.setIndex(ordered.length === 3 ? [0, 1, 2] : [0, 1, 2, 0, 2, 3]);
  return geometry;
}

// ---------------------------------------------------------------- axis triad

/**
 * The RGB axis triad, parented to the CUBE's scene so it turns with the cube
 * and therefore always shows the true export-axis directions.
 *
 * Not a pick region: it is an indicator, not a control. It is never pushed to
 * `regions`, so `pick` raycasts straight past it and the cube stays clickable
 * underneath — which is the whole reason it can be drawn on top safely.
 *
 * Layout, all from the constants at the top of the file:
 *
 *      Z
 *      │                 anchor A' sits AXIS_GAP proud of the export
 *      │                 (−X,−Y,−Z) cube corner, on all three axes at once,
 *      A'───── Y         so each arm clears the two faces it runs between
 *     ╱                  and all three still meet at a point.
 *    X
 */
function buildAxisTriad(scene: THREE.Scene): void {
  // The export (−X, −Y, −Z) corner, in display space. Scaling the corner
  // outward by the gap offsets it along all three axes at once — which is
  // exactly the clearance each arm needs from the two faces it runs between.
  const corner = new THREE.Vector3();
  for (const axis of AXES) corner.addScaledVector(EXPORT_TO_DISPLAY[axis], -CUBE_HALF);
  const anchor = corner.multiplyScalar((CUBE_HALF + AXIS_GAP) / CUBE_HALF);

  for (const axis of AXES) {
    const direction = EXPORT_TO_DISPLAY[axis];
    const color = AXIS_COLORS[axis];

    const bar = new THREE.Mesh(
      new THREE.BoxGeometry(AXIS_ARM, AXIS_THICKNESS, AXIS_THICKNESS),
      new THREE.MeshBasicMaterial({ color, depthTest: AXIS_DEPTH_TEST }),
    );
    // BoxGeometry is built along +X, so swing that onto the arm's direction.
    bar.quaternion.setFromUnitVectors(new THREE.Vector3(1, 0, 0), direction);
    bar.position.copy(anchor).addScaledVector(direction, AXIS_ARM / 2);
    // Above the cube (0), below the chrome (2): the triad annotates the cube
    // but must never sit over a control.
    bar.renderOrder = 1;
    scene.add(bar);

    // The letter is coloured in the TEXTURE, not by the material, so it can
    // carry a dark halo the material's tint would otherwise multiply away.
    const label = new THREE.Sprite(
      new THREE.SpriteMaterial({
        map: axisLetterTexture(axis, color),
        transparent: true,
        depthTest: AXIS_LABEL_DEPTH_TEST,
      }),
    );
    label.position.copy(anchor).addScaledVector(direction, AXIS_ARM + AXIS_LABEL_OFFSET);
    label.scale.setScalar(AXIS_LABEL_SCALE);
    label.renderOrder = 1;
    scene.add(label);
  }
}

/**
 * A single axis letter, in its own colour over a dark halo.
 *
 * Coloured here rather than by the material's tint, because the halo has to
 * survive: a tint multiplies the whole texture, which would turn a neutral dark
 * outline into a dark version of the axis colour and lose the contrast that
 * makes the letter readable where it floats over a pale cube face.
 */
function axisLetterTexture(letter: string, color: number): THREE.CanvasTexture {
  const size = 128;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context) {
    context.font = `700 ${size * 0.7}px ui-sans-serif, system-ui, sans-serif`;
    context.textAlign = 'center';
    context.textBaseline = 'middle';
    context.lineJoin = 'round';
    const x = size / 2;
    const y = size / 2 + size * 0.03;
    context.strokeStyle = LABEL_DARK;
    context.lineWidth = size * 0.16;
    context.strokeText(letter, x, y);
    context.fillStyle = `#${color.toString(16).padStart(6, '0')}`;
    context.fillText(letter, x, y);
  }
  return finish(canvas);
}

// ---------------------------------------------------------------- arrows

/**
 * The chrome around the cube, laid out the way Fusion lays it out:
 *
 *   - four triangles on the sides, each a 90-degree turn onto the neighbouring
 *     region;
 *   - a pair of curved arrows sweeping round the top-right corner, which ROLL
 *     the camera about its own line of sight — the one rotation the cube itself
 *     cannot express, since every cube region implies a canonical up vector;
 *   - a house at the top left, back to the default framing.
 *
 * All of it is parented to the camera, so it holds its screen position while
 * the cube turns underneath. It idles at low opacity and brightens under the
 * pointer: present when wanted, out of the way when not.
 */
function buildChrome(camera: THREE.Camera, regions: Region[]): void {
  const place = (
    texture: THREE.Texture,
    x: number,
    y: number,
    scale: number,
    rotation: number,
    pick: GizmoPick,
  ): void => {
    const material = new THREE.SpriteMaterial({
      map: texture,
      color: ARROW_COLOR,
      transparent: true,
      opacity: CHROME_IDLE_OPACITY,
      rotation,
      depthTest: false,
    });
    const sprite = new THREE.Sprite(material);
    // Camera-local: x right, y up, and negative z is in front of the lens.
    sprite.position.set(x, y, -1);
    sprite.scale.setScalar(scale);
    sprite.renderOrder = 2;
    camera.add(sprite);
    regions.push({
      object: sprite,
      pick,
      highlight: () => {
        material.opacity = CHROME_HOVER_OPACITY;
        material.color.setHex(HOVER_COLOR);
      },
      restore: () => {
        material.opacity = CHROME_IDLE_OPACITY;
        material.color.setHex(ARROW_COLOR);
      },
    });
  };

  // Same wiring as `place`, but for a shape instead of a texture on a quad.
  const sweep = (geometry: THREE.BufferGeometry, pick: GizmoPick): void => {
    const material = new THREE.MeshBasicMaterial({
      color: ARROW_COLOR,
      transparent: true,
      opacity: CHROME_IDLE_OPACITY,
      depthTest: false,
      side: THREE.DoubleSide,
    });
    const mesh = new THREE.Mesh(geometry, material);
    // Already in the camera's XY plane; only the depth has to be set.
    mesh.position.z = -1;
    mesh.renderOrder = 2;
    camera.add(mesh);
    regions.push({
      object: mesh,
      pick,
      highlight: () => {
        material.opacity = CHROME_HOVER_OPACITY;
        material.color.setHex(HOVER_COLOR);
      },
      restore: () => {
        material.opacity = CHROME_IDLE_OPACITY;
        material.color.setHex(ARROW_COLOR);
      },
    });
  };

  // Side triangles, each pointing INWARD at the cube, as Fusion draws them.
  // Outward-pointing reads as "move the camera that way"; inward reads as
  // "bring that side round to the front", which is what actually happens.
  const triangle = triangleTexture();
  const steps: Array<{ step: GizmoStep; x: number; y: number; rotation: number }> = [
    { step: 'up', x: 0, y: STEP_RADIUS, rotation: Math.PI },
    { step: 'down', x: 0, y: -STEP_RADIUS, rotation: 0 },
    { step: 'left', x: -STEP_RADIUS, y: 0, rotation: -Math.PI / 2 },
    { step: 'right', x: STEP_RADIUS, y: 0, rotation: Math.PI / 2 },
  ];
  for (const s of steps) {
    place(triangle, s.x, s.y, STEP_SCALE, s.rotation, {
      kind: 'step',
      step: s.step,
      label: `turn ${s.step}`,
    });
  }

  // Roll pair. `radians` is the way the MODEL turns on screen, which is what
  // the arrow depicts — the up vector goes the other way to deliver it. The
  // anticlockwise one takes the upper half of the band and points at the top;
  // the clockwise one takes the lower half and points at the right.
  const quarter = Math.PI / 2;
  sweep(rollGeometry(ROLL_MEET + ROLL_SPLIT, ROLL_MAX), {
    kind: 'roll',
    radians: quarter,
    label: 'roll anticlockwise',
  });
  sweep(rollGeometry(ROLL_MEET - ROLL_SPLIT, ROLL_MIN), {
    kind: 'roll',
    radians: -quarter,
    label: 'roll clockwise',
  });

  place(homeTexture(), HOME_POSITION[0], HOME_POSITION[1], HOME_SCALE, 0, {
    kind: 'home',
    label: 'home view',
  });
}

// Every sprite texture is white artwork on transparent, so the material's
// `color` can tint it — which makes the hover highlight a one-line colour swap
// rather than a second texture per control. All of them are drawn about the
// CANVAS CENTRE: sprite rotation spins the texture inside its own quad, so
// artwork that strays towards an edge gets clipped once rotated.

/** A triangle pointing up: one 90-degree step onto the neighbouring region. */
function triangleTexture(): THREE.CanvasTexture {
  const size = 128;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context) {
    const c = size / 2;
    const r = size * 0.36;
    context.fillStyle = '#ffffff';
    context.beginPath();
    context.moveTo(c, c - r);
    context.lineTo(c + r * 0.9, c + r * 0.66);
    context.lineTo(c - r * 0.9, c + r * 0.66);
    context.closePath();
    context.fill();
  }
  return finish(canvas);
}

/**
 * One roll arrow: an annulus sector from `tail` to a head that tips at `tip`,
 * drawn in the camera's own XY plane and centred on the cube.
 *
 * A MESH, not a sprite, and that is the whole point. A sprite is picked over
 * its entire quad rather than its artwork, so an arrow drawn large enough to
 * read swallows the clicks meant for the cube faces beneath it — which is what
 * sank the previous version. Raycasting a mesh matches the drawn shape exactly,
 * so these can be as big as they need to be.
 *
 * The outline runs: along the inner radius from tail to the head's base, out
 * through the inner barb, the tip, the outer barb, then back along the outer
 * radius. `ROLL_HEAD_INNER < ROLL_INNER < ROLL_OUTER < ROLL_HEAD_OUTER`, so the
 * two radial hops at the base sit on the same line without crossing and the
 * polygon stays simple enough for `ShapeGeometry` to triangulate.
 */
function rollGeometry(tail: number, tip: number): THREE.BufferGeometry {
  const direction = Math.sign(tip - tail);
  const base = tip - direction * ROLL_HEAD_SWEEP;
  const middle = (ROLL_INNER + ROLL_OUTER) / 2;
  const polar = (radius: number, angle: number): THREE.Vector2 =>
    new THREE.Vector2(Math.cos(angle) * radius, Math.sin(angle) * radius);

  const segments = 24;
  const at = (index: number): number => tail + ((base - tail) * index) / segments;
  const outline: THREE.Vector2[] = [];
  for (let i = 0; i <= segments; i++) outline.push(polar(ROLL_INNER, at(i)));
  outline.push(polar(ROLL_HEAD_INNER, base), polar(middle, tip), polar(ROLL_HEAD_OUTER, base));
  for (let i = segments; i >= 0; i--) outline.push(polar(ROLL_OUTER, at(i)));

  return new THREE.ShapeGeometry(new THREE.Shape(outline));
}

/** A house: the default framing, same idea as Fusion's home. */
function homeTexture(): THREE.CanvasTexture {
  const size = 128;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context) {
    const c = size / 2;
    const r = size * 0.34;
    context.strokeStyle = '#ffffff';
    context.lineWidth = size * 0.1;
    context.lineJoin = 'round';
    context.lineCap = 'round';
    context.beginPath();
    context.moveTo(c - r, c - r * 0.06);
    context.lineTo(c, c - r * 0.86);
    context.lineTo(c + r, c - r * 0.06);
    context.stroke();
    context.beginPath();
    context.moveTo(c - r * 0.68, c - r * 0.24);
    context.lineTo(c - r * 0.68, c + r * 0.78);
    context.lineTo(c + r * 0.68, c + r * 0.78);
    context.lineTo(c + r * 0.68, c - r * 0.24);
    context.stroke();
  }
  return finish(canvas);
}

function finish(canvas: HTMLCanvasElement): THREE.CanvasTexture {
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

/**
 * A face label — the CAD view name over the export axis — drawn dark on white
 * so the material's `color` can tint it, which is what makes the hover
 * highlight a one-line colour swap rather than a second texture per face.
 *
 * 256px rather than the 128 the glyph textures use: two lines of type on a face
 * about 39 screen pixels across needs the headroom, and mip-blurred small caps
 * turn to mush. Faces are the only textures here with fine detail.
 */
function faceTexture(name: string, axis: string): THREE.CanvasTexture {
  const size = 256;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context) {
    context.fillStyle = '#ffffff';
    context.fillRect(0, 0, size, size);
    context.strokeStyle = 'rgba(11, 13, 22, 0.35)';
    context.lineWidth = 12;
    context.strokeRect(6, 6, size - 12, size - 12);
    context.textAlign = 'center';
    context.textBaseline = 'middle';

    // BOTTOM is the widest name and would otherwise run under the border, so
    // the name line is fitted rather than assumed. Sized once, from the real
    // measurement, instead of picking a font small enough for the worst case
    // and leaving RIGHT and TOP needlessly tiny.
    //
    // FACE_TEXT_WIDTH is deliberately generous: at a 39px face, legibility beats
    // padding, so the type is allowed to run close to the border.
    context.fillStyle = LABEL_DARK;
    context.font = fitFont(context, name, size * FACE_NAME_SIZE, size * FACE_TEXT_WIDTH);
    context.fillText(name, size / 2, size * 0.395);

    context.fillStyle = LABEL_AXIS;
    context.font = fitFont(context, axis, size * FACE_AXIS_SIZE, size * FACE_TEXT_WIDTH);
    context.fillText(axis, size / 2, size * 0.67);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

/** The largest of `preferred` or smaller at which `text` fits `maxWidth`. */
function fitFont(
  context: CanvasRenderingContext2D,
  text: string,
  preferred: number,
  maxWidth: number,
): string {
  const font = (px: number): string =>
    `${FACE_FONT_WEIGHT} ${px}px ui-sans-serif, system-ui, sans-serif`;
  let px = preferred;
  context.font = font(px);
  while (px > 6 && context.measureText(text).width > maxWidth) {
    px -= 1;
    context.font = font(px);
  }
  return font(px);
}
