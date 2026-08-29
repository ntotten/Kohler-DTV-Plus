import { applyBasis, basisToZUp, scaleFactor, type LengthUnit, type UpAxis } from './units';
import type { RawGeometry } from './stl';

// Measurements taken in EXPORT space (mm, Z-up), not display space — these
// numbers are what someone reads before cutting material, so they have to match
// the file that gets downloaded rather than the picture on screen.

export interface MeshStats {
  triangleCount: number;
  vertexCount: number;
  /** Axis-aligned bounds in the target unit, export orientation. */
  min: [number, number, number];
  max: [number, number, number];
  size: [number, number, number];
  /** Enclosed volume. Meaningful only if the mesh is closed — see `closed`. */
  volume: number;
  surfaceArea: number;
  /**
   * Whether every edge is shared by exactly two triangles. A false here means
   * the volume figure is not trustworthy and a slicer may need to repair the
   * mesh before printing.
   */
  closed: boolean;
  /** Edges used by only one triangle. Non-zero implies holes. */
  boundaryEdges: number;
  unit: LengthUnit;
}

export interface MeshStatsOptions {
  sourceUnit: LengthUnit;
  sourceUpAxis: UpAxis;
  targetUnit?: LengthUnit;
}

/**
 * Compute geometry statistics in export space.
 *
 * Volume uses the signed-tetrahedron sum: for each triangle, the signed volume
 * of the tetrahedron it forms with the origin is `v0 . (v1 x v2) / 6`, and for a
 * closed, consistently-wound mesh those signed contributions telescope to the
 * enclosed volume no matter where the origin sits. On an open mesh the sum is
 * meaningless, which is why `closed` is reported alongside it rather than the
 * volume being offered on its own.
 */
export function computeMeshStats(geometry: RawGeometry, options: MeshStatsOptions): MeshStats {
  const targetUnit = options.targetUnit ?? 'mm';
  const scale = scaleFactor(options.sourceUnit, targetUnit);
  const basis = basisToZUp(options.sourceUpAxis);

  const positions =
    geometry.positions instanceof Float32Array
      ? geometry.positions
      : Float32Array.from(geometry.positions);
  const indices = geometry.indices ?? null;
  const triangleCount = indices && indices.length ? indices.length / 3 : positions.length / 9;

  const min: [number, number, number] = [Infinity, Infinity, Infinity];
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  let volume = 0;
  let surfaceArea = 0;

  // Edge use counts, for the closed-mesh test. Keyed on quantized position
  // pairs rather than vertex indices: the OBJ path arrives non-indexed, so
  // index identity does not exist, and CAD exporters routinely duplicate a
  // vertex that is geometrically shared.
  const edgeUse = new Map<string, number>();

  const tri = new Float64Array(9);

  const vertexAt = (corner: number): number => {
    if (indices && indices.length) return indices[corner] * 3;
    return corner * 3;
  };

  for (let t = 0; t < triangleCount; t++) {
    for (let corner = 0; corner < 3; corner++) {
      const p = vertexAt(t * 3 + corner);
      const [x, y, z] = applyBasis(
        basis,
        positions[p] * scale,
        positions[p + 1] * scale,
        positions[p + 2] * scale,
      );
      tri[corner * 3] = x;
      tri[corner * 3 + 1] = y;
      tri[corner * 3 + 2] = z;
      for (let axis = 0; axis < 3; axis++) {
        const value = tri[corner * 3 + axis];
        if (value < min[axis]) min[axis] = value;
        if (value > max[axis]) max[axis] = value;
      }
    }

    volume += signedTetraVolume(tri);
    surfaceArea += triangleArea(tri);
    countEdges(edgeUse, tri);
  }

  let boundaryEdges = 0;
  for (const uses of edgeUse.values()) {
    if (uses !== 2) boundaryEdges++;
  }

  const finite = Number.isFinite(min[0]);
  return {
    triangleCount,
    vertexCount: positions.length / 3,
    min: finite ? min : [0, 0, 0],
    max: finite ? max : [0, 0, 0],
    size: finite ? [max[0] - min[0], max[1] - min[1], max[2] - min[2]] : [0, 0, 0],
    volume: Math.abs(volume),
    surfaceArea,
    closed: triangleCount > 0 && boundaryEdges === 0,
    boundaryEdges,
    unit: targetUnit,
  };
}

function signedTetraVolume(t: Float64Array): number {
  const [ax, ay, az, bx, by, bz, cx, cy, cz] = t;
  // ax . (b x c) / 6
  return (ax * (by * cz - bz * cy) + ay * (bz * cx - bx * cz) + az * (bx * cy - by * cx)) / 6;
}

function triangleArea(t: Float64Array): number {
  const ux = t[3] - t[0];
  const uy = t[4] - t[1];
  const uz = t[5] - t[2];
  const vx = t[6] - t[0];
  const vy = t[7] - t[1];
  const vz = t[8] - t[2];
  return Math.hypot(uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx) / 2;
}

/**
 * Tally each undirected edge. Positions are quantized to 1e-4 mm before being
 * keyed so that two CAD vertices meant to be the same point, but differing in
 * the last float bit, still register as one shared edge. Tighter than any real
 * tolerance, loose enough to absorb float32 round-off from the source file.
 */
function countEdges(edgeUse: Map<string, number>, t: Float64Array): void {
  for (let i = 0; i < 3; i++) {
    const j = (i + 1) % 3;
    const a = key(t, i);
    const b = key(t, j);
    const edge = a < b ? `${a}|${b}` : `${b}|${a}`;
    edgeUse.set(edge, (edgeUse.get(edge) ?? 0) + 1);
  }
}

function key(t: Float64Array, corner: number): string {
  const q = (n: number) => Math.round(n * 1e4);
  return `${q(t[corner * 3])},${q(t[corner * 3 + 1])},${q(t[corner * 3 + 2])}`;
}

/** Volume in cubic centimetres, the unit filament and stock are reasoned about in. */
export function volumeCm3(stats: MeshStats): number {
  if (stats.unit !== 'mm') return NaN;
  return stats.volume / 1000;
}

/**
 * Rough print mass. Default density is PLA at 1.24 g/cm3. Solid-volume based,
 * so it is an upper bound — real prints are infilled and will weigh less.
 */
export function estimatedMassGrams(stats: MeshStats, densityGramsPerCm3 = 1.24): number {
  return volumeCm3(stats) * densityGramsPerCm3;
}
