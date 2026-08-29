import type { RawGeometry } from './stl';
import { scaleFactor, type LengthUnit } from './units';

// Mesh repair.
//
// Terminology, because the two get conflated and they are not the same fix:
//
//   WELDING (merge by distance) joins triangles that touch geometrically but
//     reference duplicate vertices, so their shared edge is counted twice. It
//     fixes CRACKS. It adds no geometry.
//   CAPPING (hole filling) triangulates a boundary loop that has nothing on the
//     other side. It fixes HOLES. It DOES add geometry.
//
// Welding first is not optional: a hole finder run on unwelded geometry sees
// every crack as a hole and would "fill" seams that were never open.
//
// What this module will not do: bridge two different loops, guess at missing
// internal structure, or re-mesh. If a loop cannot be capped honestly it is
// reported as skipped and the result is declared not watertight. A repair that
// silently invents geometry is worse than no repair, because the output looks
// authoritative.

export interface RepairOptions {
  /** Vertices closer than this merge. Default 0.001 mm. */
  weldToleranceMm?: number;
  /**
   * Maximum out-of-plane deviation for a loop to be capped with a flat face.
   * Default 0.25 mm. A loop flatter than this is capped on its best-fit plane;
   * anything more bowed is skipped rather than flattened.
   */
  planarToleranceMm?: number;
  /**
   * Collapse non-manifold edges shorter than this. Default 0.05 mm.
   *
   * Targeted, deliberately, rather than raising `weldToleranceMm`: on the
   * Kohler CAD a global 0.05 mm weld fixes the two bad edges and creates six
   * new ones elsewhere, because it also merges vertices that were legitimately
   * distinct. Collapsing only edges already known to be non-manifold, and only
   * when they are far shorter than any real feature, touches nothing else.
   *
   * Set to 0 to disable.
   */
  nonManifoldCollapseMm?: number;
  /** Refuse absurdly large loops rather than hang. Default 10000. */
  maxLoopPoints?: number;
}

export interface SkippedLoop {
  points: number;
  planarityMm: number;
  perimeterMm: number;
  reason: string;
}

export interface RepairReport {
  weldedVertices: number;
  /** Hairline non-manifold (T-junction) edges collapsed to a point. */
  collapsedEdges: number;
  boundaryEdgesBefore: number;
  boundaryEdgesAfter: number;
  nonManifoldEdges: number;
  loopsFound: number;
  loopsCapped: number;
  skipped: SkippedLoop[];
  trianglesBefore: number;
  trianglesAdded: number;
  /** True only if zero boundary edges AND zero non-manifold edges remain. */
  watertight: boolean;
}

export interface RepairResult {
  geometry: RawGeometry;
  report: RepairReport;
}

interface Vec3 {
  x: number;
  y: number;
  z: number;
}

/**
 * Weld, then cap every closed planar boundary loop.
 *
 * Operates entirely in SOURCE space and returns source-space geometry, so the
 * existing export path applies its unit and axis transform exactly as it does
 * for unrepaired geometry. Tolerances are given in millimetres and converted in,
 * because millimetres are what a person reasons about regardless of what the
 * file happens to be authored in.
 */
export function repairMesh(
  geometry: RawGeometry,
  sourceUnit: LengthUnit,
  options: RepairOptions = {},
): RepairResult {
  const toSource = scaleFactor('mm', sourceUnit);
  const weldTol = (options.weldToleranceMm ?? 0.001) * toSource;
  const planarTol = (options.planarToleranceMm ?? 0.25) * toSource;
  const maxLoopPoints = options.maxLoopPoints ?? 10000;
  const mmPerSource = scaleFactor(sourceUnit, 'mm');

  const positions = expand(geometry);
  const trianglesBefore = positions.length / 9;

  // ---- weld -----------------------------------------------------------------
  // Quantize to the weld tolerance and intern each distinct position.
  const scale = weldTol > 0 ? 1 / weldTol : 1e6;
  const index = new Map<string, number>();
  const verts: Vec3[] = [];
  const triangles: Array<[number, number, number]> = [];

  const intern = (x: number, y: number, z: number): number => {
    const key = `${Math.round(x * scale)},${Math.round(y * scale)},${Math.round(z * scale)}`;
    const hit = index.get(key);
    if (hit !== undefined) return hit;
    const id = verts.length;
    verts.push({ x, y, z });
    index.set(key, id);
    return id;
  };

  for (let t = 0; t < trianglesBefore; t++) {
    const o = t * 9;
    const a = intern(positions[o], positions[o + 1], positions[o + 2]);
    const b = intern(positions[o + 3], positions[o + 4], positions[o + 5]);
    const c = intern(positions[o + 6], positions[o + 7], positions[o + 8]);
    // A triangle whose corners welded together has no area; dropping it is part
    // of the repair, not a loss.
    if (a === b || b === c || a === c) continue;
    triangles.push([a, b, c]);
  }

  const weldedVertices = trianglesBefore * 3 - verts.length;

  // ---- collapse hairline non-manifold edges ---------------------------------
  const collapseTol = (options.nonManifoldCollapseMm ?? 0.05) * toSource;
  const collapsedEdges =
    collapseTol > 0 ? collapseShortNonManifold(triangles, verts, collapseTol) : 0;

  // ---- find boundary --------------------------------------------------------
  // Only the count AFTER repair is reported; this pass just locates the holes.
  const { boundary } = boundaryHalfEdges(triangles);
  const boundaryEdgesBefore = boundary.length;

  // ---- walk loops -----------------------------------------------------------
  const loops = walkLoops(boundary, maxLoopPoints);

  // ---- cap ------------------------------------------------------------------
  const skipped: SkippedLoop[] = [];
  let loopsCapped = 0;
  let trianglesAdded = 0;

  for (const loop of loops) {
    const pts = loop.map((i) => verts[i]);
    const perimeterMm = perimeter(pts) * mmPerSource;

    if (loop.length < 3) {
      skipped.push({
        points: loop.length,
        planarityMm: 0,
        perimeterMm,
        reason: 'fewer than 3 points — not a closed loop',
      });
      continue;
    }

    const plane = bestFitPlane(pts);
    const deviation = maxDeviation(pts, plane);
    if (deviation > planarTol) {
      skipped.push({
        points: loop.length,
        planarityMm: deviation * mmPerSource,
        perimeterMm,
        reason: 'loop is not planar within tolerance — flattening it would invent geometry',
      });
      continue;
    }

    const capTris = triangulateLoop(pts, plane);
    if (!capTris.length) {
      skipped.push({
        points: loop.length,
        planarityMm: deviation * mmPerSource,
        perimeterMm,
        reason: 'ear clipping failed — loop is self-intersecting or degenerate',
      });
      continue;
    }

    // Orientation, decided from topology rather than from a plane-normal
    // convention. The loop was walked along half-edges in the direction their
    // owning triangles wind them, so a cap must traverse each shared edge the
    // OTHER way — that is what makes the edge manifold instead of doubled.
    //
    // Deriving it from the Newell normal instead is a trap: `triangulateLoop`
    // normalizes its output to counter-clockwise in the plane's own basis, and
    // that basis is itself built from the Newell normal, so the two cancel and
    // the cap silently keeps the boundary's winding. That inverts the cap, which
    // still reports as watertight and only shows up as a wrong volume.
    const flip = capMatchesBoundary(capTris, loop.length);
    for (const [i, j, k] of capTris) {
      triangles.push(flip ? [loop[i], loop[k], loop[j]] : [loop[i], loop[j], loop[k]]);
    }
    trianglesAdded += capTris.length;
    loopsCapped++;
  }

  // ---- re-check -------------------------------------------------------------
  const after = boundaryHalfEdges(triangles);

  const out = new Float32Array(triangles.length * 9);
  triangles.forEach(([a, b, c], t) => {
    const o = t * 9;
    const va = verts[a];
    const vb = verts[b];
    const vc = verts[c];
    out[o] = va.x;
    out[o + 1] = va.y;
    out[o + 2] = va.z;
    out[o + 3] = vb.x;
    out[o + 4] = vb.y;
    out[o + 5] = vb.z;
    out[o + 6] = vc.x;
    out[o + 7] = vc.y;
    out[o + 8] = vc.z;
  });

  return {
    geometry: { positions: out },
    report: {
      weldedVertices,
      collapsedEdges,
      boundaryEdgesBefore,
      boundaryEdgesAfter: after.boundary.length,
      nonManifoldEdges: after.nonManifoldEdges,
      loopsFound: loops.length,
      loopsCapped,
      skipped,
      trianglesBefore,
      trianglesAdded,
      watertight: after.boundary.length === 0 && after.nonManifoldEdges === 0,
    },
  };
}

/**
 * Merge the endpoints of any non-manifold edge shorter than `tolerance`,
 * rewriting `triangles` in place. Returns how many edges were collapsed.
 *
 * These are T-junction seams: two surface strips that meet along a hairline
 * edge, so four triangles claim it instead of two. Collapsing the edge to a
 * single point makes the four meet at a vertex instead, which is manifold.
 * The moved distance is below the tolerance, so no surface shifts measurably.
 */
function collapseShortNonManifold(
  triangles: Array<[number, number, number]>,
  verts: Vec3[],
  tolerance: number,
): number {
  const use = new Map<string, number>();
  for (const [a, b, c] of triangles) {
    for (const [u, v] of [
      [a, b],
      [b, c],
      [c, a],
    ] as Array<[number, number]>) {
      const key = u < v ? `${u}_${v}` : `${v}_${u}`;
      use.set(key, (use.get(key) ?? 0) + 1);
    }
  }

  // Union-find, so a chain of collapses resolves to one representative.
  const parent = verts.map((_, i) => i);
  const find = (i: number): number => {
    while (parent[i] !== i) {
      parent[i] = parent[parent[i]];
      i = parent[i];
    }
    return i;
  };

  let collapsed = 0;
  for (const [key, count] of use) {
    if (count <= 2) continue;
    const [u, v] = key.split('_').map(Number);
    const a = verts[u];
    const b = verts[v];
    if (Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z) > tolerance) continue;

    const ru = find(u);
    const rv = find(v);
    if (ru === rv) continue;
    parent[rv] = ru;
    collapsed++;
  }

  if (collapsed === 0) return 0;

  // Rewrite, dropping any triangle that lost its area to the collapse.
  let write = 0;
  for (let read = 0; read < triangles.length; read++) {
    const [a, b, c] = triangles[read];
    const ra = find(a);
    const rb = find(b);
    const rc = find(c);
    if (ra === rb || rb === rc || ra === rc) continue;
    triangles[write++] = [ra, rb, rc];
  }
  triangles.length = write;
  return collapsed;
}

/**
 * True when the cap's triangles wind the same way round the boundary as the
 * existing triangles do — meaning they must be flipped before being added.
 *
 * Loop position `i` is followed by `i + 1`, so the existing surface traverses
 * `i -> i+1`. Finding that same directed pair in a cap triangle means the cap
 * agrees with the boundary and therefore faces the wrong way. Only edges that
 * lie ON the loop are conclusive; interior diagonals are ignored.
 */
function capMatchesBoundary(capTris: Array<[number, number, number]>, loopLength: number): boolean {
  const isForward = (a: number, b: number) => (a + 1) % loopLength === b;
  const isBackward = (a: number, b: number) => (b + 1) % loopLength === a;

  for (const [i, j, k] of capTris) {
    for (const [a, b] of [
      [i, j],
      [j, k],
      [k, i],
    ] as Array<[number, number]>) {
      if (isForward(a, b)) return true;
      if (isBackward(a, b)) return false;
    }
  }
  // No boundary edge appeared — degenerate, but flipping is the safer default
  // than silently trusting the plane normal.
  return false;
}

// ---------------------------------------------------------------- topology

/** Half-edges used exactly once, plus a count of edges used more than twice. */
function boundaryHalfEdges(triangles: Array<[number, number, number]>): {
  boundary: Array<[number, number]>;
  nonManifoldEdges: number;
} {
  const use = new Map<string, number>();
  const directed: Array<[number, number]> = [];

  for (const [a, b, c] of triangles) {
    for (const [u, v] of [
      [a, b],
      [b, c],
      [c, a],
    ] as Array<[number, number]>) {
      const key = u < v ? `${u}_${v}` : `${v}_${u}`;
      use.set(key, (use.get(key) ?? 0) + 1);
      directed.push([u, v]);
    }
  }

  const boundary = directed.filter(([u, v]) => {
    const key = u < v ? `${u}_${v}` : `${v}_${u}`;
    return use.get(key) === 1;
  });

  let nonManifoldEdges = 0;
  for (const count of use.values()) if (count > 2) nonManifoldEdges++;

  return { boundary, nonManifoldEdges };
}

/** Chain boundary half-edges head-to-tail into closed loops. */
function walkLoops(boundary: Array<[number, number]>, maxPoints: number): number[][] {
  const outgoing = new Map<number, number[]>();
  for (const [u, v] of boundary) {
    const list = outgoing.get(u) ?? [];
    list.push(v);
    outgoing.set(u, list);
  }

  const used = new Set<string>();
  const loops: number[][] = [];

  for (const [start, first] of boundary) {
    if (used.has(`${start}_${first}`)) continue;
    used.add(`${start}_${first}`);

    const loop = [start];
    let current = first;
    let guard = 0;

    while (current !== start && guard++ < maxPoints) {
      loop.push(current);
      const next = (outgoing.get(current) ?? []).find((n) => !used.has(`${current}_${n}`));
      if (next === undefined) break;
      used.add(`${current}_${next}`);
      current = next;
    }
    // Only a walk that returned to its start is a closed loop. An open chain
    // means the boundary is not a clean loop and must not be capped.
    if (current === start) loops.push(loop);
  }

  return loops;
}

// ---------------------------------------------------------------- geometry

interface Plane {
  origin: Vec3;
  normal: Vec3;
  u: Vec3;
  v: Vec3;
}

/**
 * Best-fit plane through a loop, via Newell's method.
 *
 * Newell rather than a cross product of two edges: it uses every vertex, so a
 * loop with a few near-collinear points still yields a stable normal instead of
 * one dominated by whichever pair happened to be picked.
 */
function bestFitPlane(points: Vec3[]): Plane {
  let nx = 0;
  let ny = 0;
  let nz = 0;
  const origin = { x: 0, y: 0, z: 0 };

  for (let i = 0; i < points.length; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    nx += (a.y - b.y) * (a.z + b.z);
    ny += (a.z - b.z) * (a.x + b.x);
    nz += (a.x - b.x) * (a.y + b.y);
    origin.x += a.x;
    origin.y += a.y;
    origin.z += a.z;
  }

  const len = Math.hypot(nx, ny, nz) || 1;
  const normal = { x: nx / len, y: ny / len, z: nz / len };
  origin.x /= points.length;
  origin.y /= points.length;
  origin.z /= points.length;

  // Any vector not parallel to the normal works as a seed for the in-plane basis.
  const seed = Math.abs(normal.x) < 0.9 ? { x: 1, y: 0, z: 0 } : { x: 0, y: 1, z: 0 };
  const u = normalize(cross(seed, normal));
  const v = cross(normal, u);
  return { origin, normal, u, v };
}

function maxDeviation(points: Vec3[], plane: Plane): number {
  let worst = 0;
  for (const p of points) {
    const d = Math.abs(
      (p.x - plane.origin.x) * plane.normal.x +
        (p.y - plane.origin.y) * plane.normal.y +
        (p.z - plane.origin.z) * plane.normal.z,
    );
    if (d > worst) worst = d;
  }
  return worst;
}

function perimeter(points: Vec3[]): number {
  let total = 0;
  for (let i = 0; i < points.length; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    total += Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
  }
  return total;
}

/**
 * Ear-clipping triangulation of a closed planar loop, returning index triples
 * into the supplied point array.
 *
 * The loop is projected onto its own plane first, so this is plain 2D ear
 * clipping. Winding is normalized to counter-clockwise in that 2D frame, then
 * the caller's reversal of the loop puts the resulting normals on the outside.
 */
function triangulateLoop(points: Vec3[], plane: Plane): Array<[number, number, number]> {
  const n = points.length;
  if (n < 3) return [];

  const flat = points.map((p) => {
    const dx = p.x - plane.origin.x;
    const dy = p.y - plane.origin.y;
    const dz = p.z - plane.origin.z;
    return {
      x: dx * plane.u.x + dy * plane.u.y + dz * plane.u.z,
      y: dx * plane.v.x + dy * plane.v.y + dz * plane.v.z,
    };
  });

  const indices = flat.map((_, i) => i);
  if (signedArea(flat) < 0) indices.reverse();

  const out: Array<[number, number, number]> = [];
  let guard = indices.length * indices.length + 16;

  while (indices.length > 3 && guard-- > 0) {
    let clipped = false;

    for (let i = 0; i < indices.length; i++) {
      const prev = indices[(i - 1 + indices.length) % indices.length];
      const curr = indices[i];
      const next = indices[(i + 1) % indices.length];

      if (!isConvex(flat[prev], flat[curr], flat[next])) continue;
      if (containsAny(flat, indices, prev, curr, next)) continue;

      out.push([prev, curr, next]);
      indices.splice(i, 1);
      clipped = true;
      break;
    }

    // No ear found: the loop self-intersects or is degenerate. Bail rather than
    // emit a fan that would cross the boundary.
    if (!clipped) return [];
  }

  if (indices.length === 3) {
    out.push([indices[0], indices[1], indices[2]]);
  }
  return out;
}

interface Vec2 {
  x: number;
  y: number;
}

function signedArea(points: Vec2[]): number {
  let total = 0;
  for (let i = 0; i < points.length; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    total += a.x * b.y - b.x * a.y;
  }
  return total / 2;
}

function isConvex(a: Vec2, b: Vec2, c: Vec2): boolean {
  return (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) > 0;
}

function containsAny(
  flat: Vec2[],
  indices: number[],
  prev: number,
  curr: number,
  next: number,
): boolean {
  for (const i of indices) {
    if (i === prev || i === curr || i === next) continue;
    if (pointInTriangle(flat[i], flat[prev], flat[curr], flat[next])) return true;
  }
  return false;
}

function pointInTriangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2): boolean {
  const d1 = sign(p, a, b);
  const d2 = sign(p, b, c);
  const d3 = sign(p, c, a);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
}

function sign(p: Vec2, a: Vec2, b: Vec2): number {
  return (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return {
    x: a.y * b.z - a.z * b.y,
    y: a.z * b.x - a.x * b.z,
    z: a.x * b.y - a.y * b.x,
  };
}

function normalize(v: Vec3): Vec3 {
  const len = Math.hypot(v.x, v.y, v.z) || 1;
  return { x: v.x / len, y: v.y / len, z: v.z / len };
}

function expand(geometry: RawGeometry): Float32Array {
  const positions =
    geometry.positions instanceof Float32Array
      ? geometry.positions
      : Float32Array.from(geometry.positions);
  const indices = geometry.indices;
  if (!indices || indices.length === 0) return positions;

  const out = new Float32Array(indices.length * 3);
  for (let i = 0; i < indices.length; i++) {
    const src = indices[i] * 3;
    out[i * 3] = positions[src];
    out[i * 3 + 1] = positions[src + 1];
    out[i * 3 + 2] = positions[src + 2];
  }
  return out;
}

/** One-line summary for a status bar. */
export function summarizeRepair(report: RepairReport): string {
  if (report.watertight) {
    const collapsed = report.collapsedEdges
      ? `, collapsed ${report.collapsedEdges} hairline seam(s)`
      : '';
    return (
      `Repaired: welded ${report.weldedVertices.toLocaleString()} vertices, ` +
      `capped ${report.loopsCapped} hole(s) with ${report.trianglesAdded} triangles` +
      `${collapsed}. Watertight and manifold.`
    );
  }
  return (
    `Partially repaired: capped ${report.loopsCapped} of ${report.loopsFound} hole(s). ` +
    `${report.boundaryEdgesAfter} boundary edge(s) and ${report.nonManifoldEdges} ` +
    `non-manifold edge(s) remain — NOT watertight.`
  );
}
