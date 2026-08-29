// Length units and up-axis remapping.
//
// This module is the whole reason the viewer can be trusted to emit a printable
// STL. Everything here is pure and unit-tested, because the two ways to hand a
// CAM operator a useless file are (a) the wrong scale and (b) the wrong axis,
// and neither is visible in the viewport — a part looks identical whether it is
// 5 inches or 5 millimetres across until you try to cut it.
//
// The convention this app fixes on:
//
//   CAD/export space : millimetres, Z-up   (what STL consumers assume)
//   Display space    : whatever, Y-up      (what three.js assumes)
//
// A source asset declares its own units and up-axis in the catalog; every
// transform is derived from that declaration rather than eyeballed.

export type LengthUnit = 'mm' | 'cm' | 'm' | 'in' | 'ft';
export type UpAxis = 'x' | 'y' | 'z';

export const LENGTH_UNITS: readonly LengthUnit[] = ['mm', 'cm', 'm', 'in', 'ft'];
export const UP_AXES: readonly UpAxis[] = ['x', 'y', 'z'];

/**
 * Millimetres per one unit. `in` is exact by definition (the international inch
 * has been exactly 25.4 mm since 1959), so no rounding creeps into a part that
 * has to mate with real hardware.
 */
export const MM_PER_UNIT: Record<LengthUnit, number> = {
  mm: 1,
  cm: 10,
  m: 1000,
  in: 25.4,
  ft: 304.8,
};

export const UNIT_LABEL: Record<LengthUnit, string> = {
  mm: 'mm',
  cm: 'cm',
  m: 'm',
  in: 'in',
  ft: 'ft',
};

export function isLengthUnit(value: unknown): value is LengthUnit {
  return typeof value === 'string' && (LENGTH_UNITS as readonly string[]).includes(value);
}

export function isUpAxis(value: unknown): value is UpAxis {
  return typeof value === 'string' && (UP_AXES as readonly string[]).includes(value);
}

/** Multiplier that converts a length expressed in `from` into `to`. */
export function scaleFactor(from: LengthUnit, to: LengthUnit): number {
  return MM_PER_UNIT[from] / MM_PER_UNIT[to];
}

/** Convert a single length between units. */
export function convertLength(value: number, from: LengthUnit, to: LengthUnit): number {
  return value * scaleFactor(from, to);
}

/**
 * A column-major 3x3 basis (three.js Matrix3 order) that rotates a model whose
 * up axis is `from` into a Z-up frame.
 *
 * Every one of these is a proper rotation (determinant +1), NOT a mirror. That
 * matters: a determinant of -1 would flip the part's handedness, and a mirrored
 * bracket fits nothing. `remapToZUp.test.ts` pins the determinant for exactly
 * this reason.
 */
export function basisToZUp(from: UpAxis): number[] {
  switch (from) {
    // Already Z-up. Identity.
    case 'z':
      return [1, 0, 0, 0, 1, 0, 0, 0, 1];
    // Y-up -> Z-up:  (x, y, z) -> (x, -z, y).   +Y lands on +Z.
    case 'y':
      return [1, 0, 0, 0, 0, 1, 0, -1, 0];
    // X-up -> Z-up:  (x, y, z) -> (-z, y, x).   +X lands on +Z.
    case 'x':
      return [0, 0, 1, 0, 1, 0, -1, 0, 0];
  }
}

/** Apply `basisToZUp` to a single point, returned as a fresh triple. */
export function applyBasis(
  basis: number[],
  x: number,
  y: number,
  z: number,
): [number, number, number] {
  // Column-major: basis[col * 3 + row].
  return [
    basis[0] * x + basis[3] * y + basis[6] * z,
    basis[1] * x + basis[4] * y + basis[7] * z,
    basis[2] * x + basis[5] * y + basis[8] * z,
  ];
}

/** Determinant of a column-major 3x3. Used to assert we never mirror a part. */
export function determinant3(m: number[]): number {
  const [a, b, c, d, e, f, g, h, i] = m;
  return a * (e * i - f * h) - d * (b * i - c * h) + g * (b * f - c * e);
}

/**
 * The rotation, in radians about X, that puts a `from`-up model upright in
 * three.js's Y-up world. Display-only — it must never leak into an export.
 *
 * A Z-up CAD model (the common case, and what the Kohler assets are) needs
 * -90 degrees; a Y-up model is already correct.
 */
export function displayRotationX(from: UpAxis): number {
  return from === 'z' ? -Math.PI / 2 : 0;
}

/**
 * The Z rotation that accompanies `displayRotationX` for an X-up source, so all
 * three cases end up genuinely upright rather than merely un-rotated.
 */
export function displayRotationZ(from: UpAxis): number {
  return from === 'x' ? Math.PI / 2 : 0;
}

/**
 * Row-major 16 elements for `THREE.Matrix4.set()` mapping source space to
 * DISPLAY space: millimetres, Y-up.
 *
 * Composed as C . B . s, where `s` scales the source unit to millimetres, `B`
 * is `basisToZUp(from)`, and `C` is the fixed Z-up-to-Y-up step
 * `(x, y, z) -> (x, z, -y)`.
 *
 * It lives here, in the pure and tested layer, rather than inline in the scene
 * code, because it is the one transform with no cross-check downstream: an
 * export error shows up in `verify-exports`, but a display error just makes the
 * part sit at a strange angle, which is easy to accept as normal.
 */
export function displayMatrixElements(from: UpAxis, sourceUnit: LengthUnit): number[] {
  const s = scaleFactor(sourceUnit, 'mm');
  const b = basisToZUp(from);
  // Rows of B (column-major storage): row r is [b[r], b[3+r], b[6+r]].
  // C keeps row 0, promotes B's row 2 to row 1, and negates B's row 1 into row 2.
  return [
    b[0] * s,
    b[3] * s,
    b[6] * s,
    0,
    b[2] * s,
    b[5] * s,
    b[8] * s,
    0,
    -b[1] * s,
    -b[4] * s,
    -b[7] * s,
    0,
    0,
    0,
    0,
    1,
  ];
}

/**
 * Row-major 16 elements mapping EXPORT space (mm, Z-up) to DISPLAY space
 * (mm, Y-up): `(x, y, z) -> (x, z, -y)`.
 *
 * This is the `C` step of `displayMatrixElements` on its own, and it is what
 * anything authored in export coordinates — a decal anchor, a measurement
 * annotation, a saved section plane — needs in order to be drawn in the
 * viewport. Such things must never be authored in display space: display space
 * is a rendering convenience that can be changed, export space is the frame the
 * part is actually measured and cut in.
 *
 * `exportToDisplay . (source -> export) === displayMatrixElements` is asserted
 * in the tests, so the two can never drift apart.
 */
export function exportToDisplayMatrixElements(): number[] {
  return [1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1];
}

/** Apply a row-major 4x4 (translation-free) to a point. Test/inspection helper. */
export function applyMatrixElements(
  m: number[],
  x: number,
  y: number,
  z: number,
): [number, number, number] {
  return [
    m[0] * x + m[1] * y + m[2] * z,
    m[4] * x + m[5] * y + m[6] * z,
    m[8] * x + m[9] * y + m[10] * z,
  ];
}

/** Human-readable dimension string, e.g. `133.6 x 30.8 x 84.1 mm`. */
export function formatDimensions(size: readonly number[], unit: LengthUnit, precision = 1): string {
  const [x, y, z] = size;
  const f = (n: number) => n.toFixed(precision);
  return `${f(x)} x ${f(y)} x ${f(z)} ${UNIT_LABEL[unit]}`;
}
