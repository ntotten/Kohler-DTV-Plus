import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import {
  applyBasis,
  basisToZUp,
  displayMatrixElements,
  scaleFactor,
  type LengthUnit,
  type UpAxis,
} from '../core/units';
import { disposeObject, extractSourceGeometry } from './loaders';
import {
  applyPose,
  capturePose,
  frameObject,
  goHome,
  rollCamera,
  snapToDirection,
  snapToView,
  type ViewName,
} from './cameraFit';
import { createCameraTween } from './cameraTween';
import { createDecalLayer, type DecalHandle } from './decalLayer';
import { createViewGizmo, stepDirection, type GizmoPick } from './viewGizmo';
import { createDragProbe, dragThresholdPx, type DragProbe } from './gizmoDrag';
import type { RawGeometry } from '../core/stl';
import type { DecalRecord } from '../core/decals';

// The WebGL engine. Owns GPU resources; knows nothing about the catalog or the
// DOM chrome around it.
//
// Display space is DELIBERATELY millimetres, Y-up:
//   * millimetres so the grid squares are a real 10 mm and the pointer readout
//     needs no further conversion;
//   * Y-up because that is three's world and fighting it costs more than the
//     one rotation it saves.
// Export space stays millimetres Z-up. `toExportSpace` is the only bridge
// between the two, and it inverts the display transform rather than guessing.

const CAMERA_FOV = 45;
const MAX_PIXEL_RATIO = 2;
const BACKGROUND_COLOR = 0x0b0d16;
const MODEL_COLOR = 0xb8c1d9;
const GRID_COLOR_CENTER = 0x3a4a5a;
const GRID_COLOR_LINES = 0x232838;
const GRID_OPACITY = 0.35;
const GRID_CELL_MM = 10;
const GRID_MIN_CELLS = 12;
const BBOX_COLOR = 0x4d7cff;
const HIGHLIGHT_COLOR = 0xffb347;
/**
 * How much faster dragging the CUBE orbits than dragging the model, as a
 * multiplier on OrbitControls' `rotateSpeed`.
 *
 * The two surfaces are not the same size and should not have the same gearing.
 * Dragging the model, your pointer has the whole viewport to travel in and a
 * 1:1 feel is right — you are pushing the part around and want to be able to
 * stop on a precise angle. The cube is a 132 px square in the corner: at 1:1 a
 * full turn needs the pointer to cross most of the window, which means letting
 * go of the cube, and it stops being a control you can flick.
 *
 * At 2x, a half-turn is about a third of the canvas height, which keeps the
 * whole gesture inside comfortable reach of the widget. The click threshold is
 * in PIXELS and so is unaffected — a click is still a click at the same travel,
 * it is only the rotation each pixel buys that changes.
 */
const GIZMO_DRAG_ROTATE_SPEED = 5;

export interface LoadedModel {
  /** Untouched source-space triangle soup. Measurement and export read this. */
  sourceGeometry: RawGeometry;
  sourceUnit: LengthUnit;
  sourceUpAxis: UpAxis;
}

export interface PickResult {
  /** Point in export space: millimetres, Z-up. */
  point: [number, number, number];
  distanceMm: number;
}

export interface ViewerHandles {
  setModel(object: THREE.Object3D, unit: LengthUnit, upAxis: UpAxis): LoadedModel;
  clearModel(): void;
  fit(): void;
  snap(view: ViewName): void;
  setWireframe(on: boolean): void;
  setFlatShading(on: boolean): void;
  setGridVisible(on: boolean): void;
  setBoundsVisible(on: boolean): void;
  /**
   * Put decals on the scene. They are added to their own root, never to the
   * model, so they cannot reach the export path — see `decalLayer.ts`.
   */
  setDecals(records: DecalRecord[]): Promise<DecalHandle[]>;
  selectDecal(decalId: string | null): void;
  /**
   * Look at a decal square-on and the right way up.
   *
   * Both vectors are EXPORT space. The up vector is the anchor's own `v`, so
   * "upright" means upright as the artwork was authored — which, for a part
   * modelled on its side, is the only frame that knows which way is up.
   */
  lookAtDecal(normal: readonly number[], up: readonly number[]): void;
  /** Screen-space pick, returning export-space millimetres. */
  pick(clientX: number, clientY: number): PickResult | null;
  dispose(): void;
}

export function createViewer(canvas: HTMLCanvasElement, container: HTMLElement): ViewerHandles {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(BACKGROUND_COLOR);

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, MAX_PIXEL_RATIO));

  const camera = new THREE.PerspectiveCamera(CAMERA_FOV, 1, 0.1, 100000);
  camera.position.set(0, 0, 300);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = !prefersReducedMotion();
  controls.dampingFactor = 0.08;

  scene.add(new THREE.HemisphereLight(0xffffff, 0x30354a, 1.0));
  const key = new THREE.DirectionalLight(0xffffff, 1.6);
  key.position.set(1, 1.5, 1);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xffffff, 0.6);
  fill.position.set(-1, -0.5, -1);
  scene.add(fill);

  const material = new THREE.MeshStandardMaterial({
    color: MODEL_COLOR,
    metalness: 0.15,
    roughness: 0.7,
  });

  const decals = createDecalLayer();
  scene.add(decals.root);

  // Orientation gizmo, drawn into the corner of the same canvas. Labelled with
  // EXPORT axes, so it agrees with the pointer readout rather than with the
  // viewport's own Y-up convention.
  const gizmo = createViewGizmo();

  // Gizmo moves are animated. Reduced motion collapses the duration to zero,
  // which the tween treats as "land on the next frame" rather than as a special
  // case — the same signal already governs OrbitControls' damping above.
  const tween = createCameraTween(prefersReducedMotion() ? { durationMs: 0 } : {});
  // A drag beats an animation: if the user grabs the model mid-turn, the turn
  // is abandoned rather than fighting them for the camera.
  controls.addEventListener('start', () => tween.cancel());

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();

  /** Wraps the loaded model and carries the source -> display transform. */
  let modelRoot: THREE.Group | null = null;
  let grid: THREE.GridHelper | null = null;
  let bounds: THREE.Box3Helper | null = null;
  let loaded: LoadedModel | null = null;
  let frameId = 0;
  let disposed = false;

  function clearModel(): void {
    if (modelRoot) {
      scene.remove(modelRoot);
      disposeObject(modelRoot);
      modelRoot = null;
    }
    if (grid) {
      scene.remove(grid);
      grid.geometry.dispose();
      (grid.material as THREE.Material).dispose();
      grid = null;
    }
    if (bounds) {
      scene.remove(bounds);
      bounds.geometry.dispose();
      (bounds.material as THREE.Material).dispose();
      bounds = null;
    }
    loaded = null;
  }

  function setModel(object: THREE.Object3D, unit: LengthUnit, upAxis: UpAxis): LoadedModel {
    clearModel();
    // Decals are pinned to one specific model file's geometry. Carrying them
    // across a model change would put a K-99693 faceplate onto whatever the
    // user just dropped on the window.
    decals.clear();

    // Snapshot the geometry BEFORE any display transform touches it. Everything
    // measured or exported comes from here, never from the scene graph.
    const sourceGeometry = extractSourceGeometry(object);
    loaded = { sourceGeometry, sourceUnit: unit, sourceUpAxis: upAxis };

    object.traverse((child) => {
      const mesh = child as THREE.Mesh;
      if (mesh.isMesh) mesh.material = material;
    });

    // Source -> display: scale into millimetres and rotate the declared up axis
    // onto +Y. The elements come from `displayMatrixElements`, which is pure and
    // unit-tested; building it here inline is how a shear or a mirror sneaks in
    // unnoticed, since a wrongly-oriented part just looks like a wrongly-
    // oriented part.
    modelRoot = new THREE.Group();
    const m = new THREE.Matrix4();
    m.set(...(displayMatrixElements(upAxis, unit) as Parameters<THREE.Matrix4['set']>));
    modelRoot.applyMatrix4(m);
    modelRoot.add(object);
    scene.add(modelRoot);

    addGrid();
    addBounds();
    frameObject(modelRoot, camera, controls);
    return loaded;
  }

  /** A real 10 mm grid, sized to sit comfortably under the part. */
  function addGrid(): void {
    if (!modelRoot) return;
    const box = new THREE.Box3().setFromObject(modelRoot);
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    const footprint = Math.max(size.x, size.z, GRID_CELL_MM * GRID_MIN_CELLS);
    const cells = Math.max(GRID_MIN_CELLS, Math.ceil((footprint * 1.6) / GRID_CELL_MM));
    const extent = cells * GRID_CELL_MM;

    grid = new THREE.GridHelper(extent, cells, GRID_COLOR_CENTER, GRID_COLOR_LINES);
    grid.position.set(center.x, box.min.y, center.z);
    const gridMaterial = grid.material as THREE.Material;
    gridMaterial.transparent = true;
    gridMaterial.opacity = GRID_OPACITY;
    grid.visible = true;
    scene.add(grid);
  }

  function addBounds(): void {
    if (!modelRoot) return;
    const box = new THREE.Box3().setFromObject(modelRoot);
    bounds = new THREE.Box3Helper(box, new THREE.Color(BBOX_COLOR));
    bounds.visible = false;
    scene.add(bounds);
  }

  /**
   * Convert a display-space point back to export space (mm, Z-up).
   *
   * Inverts the model root's actual matrix to recover source coordinates, then
   * re-applies the export transform. Deriving it from the live matrix rather
   * than re-deriving from unit/axis means a display change can never silently
   * desynchronise the readout from the geometry.
   */
  function toExportSpace(displayPoint: THREE.Vector3): [number, number, number] {
    if (!modelRoot || !loaded) return [0, 0, 0];
    modelRoot.updateMatrixWorld(true);
    const inverse = new THREE.Matrix4().copy(modelRoot.matrixWorld).invert();
    const source = displayPoint.clone().applyMatrix4(inverse);
    const mmScale = scaleFactor(loaded.sourceUnit, 'mm');
    return applyBasis(
      basisToZUp(loaded.sourceUpAxis),
      source.x * mmScale,
      source.y * mmScale,
      source.z * mmScale,
    );
  }

  function pick(clientX: number, clientY: number): PickResult | null {
    if (!modelRoot) return null;
    // The gizmo sits on top of the model. Reporting a coordinate from the
    // geometry hidden behind it would be a readout for a point the user cannot
    // see, so the corner is excluded.
    if (gizmo.hit(clientX, clientY, renderer)) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const hit = raycaster.intersectObject(modelRoot, true)[0];
    if (!hit) return null;
    return { point: toExportSpace(hit.point), distanceMm: hit.distance };
  }

  function resize(): void {
    const width = Math.max(container.clientWidth, 1);
    const height = Math.max(container.clientHeight, 1);
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
  }

  function renderFrame(): void {
    if (disposed) return;
    frameId = window.requestAnimationFrame(renderFrame);
    resize();
    // Before controls.update(), so damping smooths the tween's own output
    // rather than chasing a camera that has already been moved past it.
    const pose = tween.sample(performance.now());
    if (pose) applyPose(pose, camera, controls);
    controls.update();
    renderer.render(scene, camera);
    gizmo.render(renderer, camera);
  }

  // Gizmo input. Bound here rather than in `main.ts` because the hit test needs
  // the renderer's own pixel geometry, which is this module's business.
  //
  // CUBE REGIONS ARE CLICK-OR-DRAG. THE CHROME IS CLICK-ONLY.
  //
  // The cube regions defer: `pointerdown` on one does nothing at all, and the
  // event is left to reach OrbitControls, which starts its orbit as if the
  // pointer had gone down on the model. If the pointer then releases without
  // travelling (see `gizmoDrag.ts`), the orbit is undone and the snap runs
  // instead. Drag to orbit, click to snap, with OrbitControls' own feel for the
  // drag rather than a second implementation of it.
  //
  // The chrome — step triangles, roll arrows, home — keeps the old behaviour
  // exactly: the event is claimed on `pointerdown` and acted on immediately.
  // A drag that begins on a button is ambiguous in a way a drag on the cube is
  // not. The cube face under your finger IS the thing you are turning, so
  // turning it further is the obvious reading; "roll 90 degrees" is a discrete
  // command with no continuous form to slide into, and the four step triangles
  // sit far enough out that a drag through them would have to start ON one to
  // be affected. Deferring them would buy nothing and cost the immediacy.
  //
  // A cube drag is also GEARED UP — see `GIZMO_DRAG_ROTATE_SPEED`.
  const canvasElement = renderer.domElement;

  /**
   * The controls' own rotation gearing, captured before the gizmo ever touches
   * it, so restoring cannot drift towards a hard-coded 1 if the default is ever
   * tuned.
   */
  const baseRotateSpeed = controls.rotateSpeed;

  /** A gesture that began on a cube region and has not yet been resolved. */
  let gesture: { pointerId: number; pick: GizmoPick; probe: DragProbe } | null = null;

  /**
   * End the current cube gesture and put back everything it changed.
   *
   * Every exit runs through here — release, cancel, and disposal — because the
   * gearing is global to the controls. Leaving it doubled after a gesture that
   * ended somewhere unexpected would make dragging the MODEL twice as fast for
   * the rest of the session, which is the kind of bug that gets blamed on the
   * mouse.
   */
  function endGesture(): void {
    gesture = null;
    controls.rotateSpeed = baseRotateSpeed;
    canvasElement.style.cursor = '';
  }

  canvasElement.addEventListener(
    'pointerdown',
    (event) => {
      const pick = gizmo.hit(event.clientX, event.clientY, renderer);
      if (!pick || !modelRoot) return;

      if (pick.kind !== 'view') {
        event.stopPropagation();
        event.preventDefault();
        applyGizmoPick(pick);
        return;
      }

      // Deliberately NOT stopped or prevented: OrbitControls has to see this
      // pointerdown to start its drag from the right place, and its `start`
      // event is what cancels an in-flight tween.
      gesture = {
        pointerId: event.pointerId,
        pick,
        probe: createDragProbe(event.clientX, event.clientY, dragThresholdPx(event.pointerType)),
      };
      // Geared up for the whole gesture, including the pixels before the drag
      // is confirmed — those either become part of the orbit, in which case
      // they should already be at the cube's rate, or the gesture turns out to
      // be a click and the rotation they bought is discarded anyway.
      controls.rotateSpeed = baseRotateSpeed * GIZMO_DRAG_ROTATE_SPEED;
      // OrbitControls captures the pointer itself, but only for the first one
      // down. Capturing here too means the `pointerup` that ends the gesture
      // arrives even if the pointer has left the canvas, whatever the controls
      // decide to do with it.
      canvasElement.setPointerCapture(event.pointerId);
    },
    { capture: true },
  );

  canvasElement.addEventListener('pointerup', (event) => {
    const active = gesture;
    if (!active || active.pointerId !== event.pointerId) return;
    endGesture();
    // It travelled: OrbitControls has already delivered the orbit the user
    // asked for, and there is nothing left to do.
    if (!active.probe.isClick) return;
    stopControlsMomentum();
    applyGizmoPick(active.pick);
  });

  canvasElement.addEventListener('pointercancel', (event) => {
    if (gesture?.pointerId !== event.pointerId) return;
    endGesture();
  });

  /**
   * Throw away the rotation OrbitControls has queued but not yet applied.
   *
   * THIS IS THE PRICE OF LETTING THE EVENT THROUGH, and without it a click on
   * the cube lands on the wrong view. A click is a click because it travelled
   * under a few pixels — but a few pixels is still a couple of degrees of
   * orbit, and with damping on, OrbitControls applies only `dampingFactor` of
   * that per frame and keeps the remainder queued. `renderFrame` applies the
   * tween's pose and THEN calls `controls.update()`, so every frame of the
   * animation would have the leftover rotation added on top of it. The snap and
   * the damping would spend the whole 340 ms fighting over the camera, and the
   * view would settle a degree or two off the axis the user clicked — which is
   * the one thing an axis-aligned view must not be.
   *
   * Zeroing the queue discards it outright, so nothing jumps: the tween starts
   * from where the camera visibly is and lands exactly on the destination.
   *
   * `_sphericalDelta` and `_panOffset` are underscore-prefixed, so this reaches
   * past three's public API on purpose — the same bargain, and the same guard,
   * as `syncControlsUp` in `cameraFit.ts`. The fallback is not a no-op: with
   * damping off, `update()` applies the pending delta in full and then zeroes
   * it, which costs a sub-threshold flick of rotation before the tween starts
   * instead of leaving it to bleed out across the animation.
   */
  function stopControlsMomentum(): void {
    const internals = controls as unknown as {
      _sphericalDelta?: THREE.Spherical;
      _panOffset?: THREE.Vector3;
    };
    if (internals._sphericalDelta && internals._panOffset) {
      internals._sphericalDelta.set(0, 0, 0);
      internals._panOffset.set(0, 0, 0);
      return;
    }
    const damping = controls.enableDamping;
    controls.enableDamping = false;
    controls.update();
    controls.enableDamping = damping;
  }

  function applyGizmoPick(pick: GizmoPick): void {
    if (!modelRoot) return;
    // Compute the destination by running the move for real, then rewind and
    // animate into it. The alternative — a second, "predictive" copy of each
    // move — is the kind of duplication that drifts: the tween would land
    // somewhere subtly different from where a click used to, and only under
    // some rotations. This way there is one implementation of each move and
    // the animation is a pure presentation layer over it.
    const from = capturePose(camera, controls);
    runGizmoMove(pick);
    const to = capturePose(camera, controls);
    applyPose(from, camera, controls);
    tween.start(from, to, performance.now());
  }

  function runGizmoMove(pick: GizmoPick): void {
    if (!modelRoot) return;
    switch (pick.kind) {
      case 'view':
        snapToDirection(modelRoot, camera, controls, pick.towards);
        return;
      case 'step':
        snapToDirection(
          modelRoot,
          camera,
          controls,
          stepDirection(pick.step, camera, controls.target),
        );
        return;
      case 'roll':
        // Roll keeps the camera where it is and turns the picture, so it must
        // NOT go through snapToDirection — that would reset the up vector and
        // undo the roll on the spot.
        rollCamera(camera, controls, pick.radians);
        return;
      case 'home':
        goHome(modelRoot, camera, controls);
        return;
    }
  }

  canvasElement.addEventListener('pointermove', (event) => {
    if (gesture && gesture.pointerId === event.pointerId) {
      if (!gesture.probe.moved(event.clientX, event.clientY)) return;
      // Committed to an orbit. The highlight goes out and the cursor changes,
      // because the cube has stopped being a set of targets and become the
      // thing being turned — leaving a face lit under a moving pointer reads as
      // a stuck highlight.
      gizmo.clearHover();
      canvasElement.style.cursor = 'grabbing';
      return;
    }
    const handle = gizmo.hover(event.clientX, event.clientY, renderer);
    canvasElement.style.cursor = handle ? 'pointer' : '';
  });

  resize();
  renderFrame();

  return {
    setModel,
    clearModel,
    fit: () => {
      if (modelRoot) frameObject(modelRoot, camera, controls, 1.25, true);
    },
    snap: (view) => {
      if (modelRoot) snapToView(modelRoot, camera, controls, view);
    },
    setWireframe: (on) => {
      material.wireframe = on;
      material.needsUpdate = true;
    },
    setFlatShading: (on) => {
      material.flatShading = on;
      material.needsUpdate = true;
    },
    setGridVisible: (on) => {
      if (grid) grid.visible = on;
    },
    setBoundsVisible: (on) => {
      if (bounds) bounds.visible = on;
    },
    setDecals: (records) => decals.setDecals(records, renderer.capabilities.getMaxAnisotropy()),
    selectDecal: (decalId) => decals.select(decalId),
    lookAtDecal: (normal, up) => {
      if (!modelRoot) return;
      snapToDirection(modelRoot, camera, controls, exportToDisplay(normal), exportToDisplay(up));
    },
    pick,
    dispose: () => {
      disposed = true;
      endGesture();
      window.cancelAnimationFrame(frameId);
      clearModel();
      decals.dispose();
      gizmo.dispose();
      material.dispose();
      controls.dispose();
      renderer.dispose();
    },
  };
}

/** A direction in export space (mm, Z-up) as a display-space vector. */
function exportToDisplay(v: readonly number[]): THREE.Vector3 {
  return new THREE.Vector3(v[0], v[2], -v[1]).normalize();
}

export function isWebglSupported(): boolean {
  return typeof window !== 'undefined' && 'WebGLRenderingContext' in window;
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export const VIEWER_COLORS = { HIGHLIGHT_COLOR, MODEL_COLOR } as const;
