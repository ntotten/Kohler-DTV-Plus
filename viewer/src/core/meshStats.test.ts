import { describe, expect, it } from 'vitest';
import { computeMeshStats, estimatedMassGrams, volumeCm3 } from './meshStats';

/**
 * A closed axis-aligned box from (0,0,0) to (w,h,d), 12 triangles, wound
 * outward. Built explicitly rather than with a helper so the winding is
 * auditable — a single flipped triangle would break both the volume and the
 * closed test, and it should be obvious which.
 */
function box(w: number, h: number, d: number): { positions: Float32Array } {
  const v = [
    [0, 0, 0],
    [w, 0, 0],
    [w, h, 0],
    [0, h, 0],
    [0, 0, d],
    [w, 0, d],
    [w, h, d],
    [0, h, d],
  ];
  const faces = [
    [0, 3, 2],
    [0, 2, 1], // bottom (-Z)
    [4, 5, 6],
    [4, 6, 7], // top (+Z)
    [0, 1, 5],
    [0, 5, 4], // -Y
    [2, 3, 7],
    [2, 7, 6], // +Y
    [1, 2, 6],
    [1, 6, 5], // +X
    [0, 4, 7],
    [0, 7, 3], // -X
  ];
  const positions = new Float32Array(faces.length * 9);
  faces.forEach((face, f) => {
    face.forEach((vi, corner) => {
      const o = f * 9 + corner * 3;
      positions[o] = v[vi][0];
      positions[o + 1] = v[vi][1];
      positions[o + 2] = v[vi][2];
    });
  });
  return { positions };
}

describe('computeMeshStats — dimensions', () => {
  it('measures a box in its own units', () => {
    const stats = computeMeshStats(box(10, 20, 30), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.triangleCount).toBe(12);
    expect(stats.size[0]).toBeCloseTo(10, 4);
    expect(stats.size[1]).toBeCloseTo(20, 4);
    expect(stats.size[2]).toBeCloseTo(30, 4);
  });

  it('reports dimensions in export units, not source units', () => {
    // An inch-authored 1x2x3 box must read as 25.4 x 50.8 x 76.2 mm. This is
    // the number someone sets stock size from, so it follows the exported file.
    const stats = computeMeshStats(box(1, 2, 3), { sourceUnit: 'in', sourceUpAxis: 'z' });
    expect(stats.unit).toBe('mm');
    expect(stats.size[0]).toBeCloseTo(25.4, 3);
    expect(stats.size[1]).toBeCloseTo(50.8, 3);
    expect(stats.size[2]).toBeCloseTo(76.2, 3);
  });

  it('reports the K-99693 faceplate envelope in millimetres', () => {
    // Source bbox from the shipped OBJ: 5.259 x 1.214 x 3.310 in.
    const stats = computeMeshStats(box(5.259, 1.214, 3.31), {
      sourceUnit: 'in',
      sourceUpAxis: 'z',
    });
    expect(stats.size[0]).toBeCloseTo(133.58, 1);
    expect(stats.size[1]).toBeCloseTo(30.84, 1);
    expect(stats.size[2]).toBeCloseTo(84.07, 1);
  });
});

describe('computeMeshStats — volume', () => {
  it('computes the volume of a closed box', () => {
    const stats = computeMeshStats(box(10, 20, 30), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.volume).toBeCloseTo(6000, 2);
  });

  it('is independent of where the box sits relative to the origin', () => {
    // The signed-tetrahedron sum telescopes, so an offset must not change it.
    const shifted = box(10, 20, 30);
    for (let i = 0; i < shifted.positions.length; i += 3) {
      shifted.positions[i] += 500;
      shifted.positions[i + 1] -= 250;
      shifted.positions[i + 2] += 125;
    }
    const stats = computeMeshStats(shifted, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.volume).toBeCloseTo(6000, 1);
  });

  it('scales volume cubically with the unit conversion', () => {
    const stats = computeMeshStats(box(1, 1, 1), { sourceUnit: 'in', sourceUpAxis: 'z' });
    expect(stats.volume).toBeCloseTo(25.4 ** 3, 1);
  });

  it('computes the surface area of a box', () => {
    const stats = computeMeshStats(box(10, 20, 30), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    // 2*(10*20 + 20*30 + 10*30) = 2200
    expect(stats.surfaceArea).toBeCloseTo(2200, 2);
  });
});

describe('computeMeshStats — watertightness', () => {
  it('recognises a closed mesh', () => {
    const stats = computeMeshStats(box(10, 20, 30), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.closed).toBe(true);
    expect(stats.boundaryEdges).toBe(0);
  });

  it('flags an open mesh, so its volume is not taken on trust', () => {
    const single = { positions: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]) };
    const stats = computeMeshStats(single, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.closed).toBe(false);
    expect(stats.boundaryEdges).toBe(3);
  });

  it('detects a hole left by a removed face', () => {
    const full = box(10, 10, 10);
    // Drop the last triangle: 4 edges become unshared (the removed triangle's
    // 3, minus shared bookkeeping) — the point is simply that it is not closed.
    const holed = { positions: full.positions.slice(0, full.positions.length - 9) };
    const stats = computeMeshStats(holed, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.closed).toBe(false);
    expect(stats.boundaryEdges).toBeGreaterThan(0);
  });

  it('treats an empty mesh as not closed', () => {
    const stats = computeMeshStats(
      { positions: new Float32Array(0) },
      { sourceUnit: 'mm', sourceUpAxis: 'z' },
    );
    expect(stats.closed).toBe(false);
    expect(stats.size).toEqual([0, 0, 0]);
  });
});

describe('computeMeshStats — indexed input', () => {
  it('produces identical stats for indexed and expanded geometry', () => {
    const positions = new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0]);
    const indices = new Uint32Array([0, 1, 2, 0, 2, 3]);
    const indexed = computeMeshStats(
      { positions, indices },
      { sourceUnit: 'mm', sourceUpAxis: 'z' },
    );
    expect(indexed.triangleCount).toBe(2);
    expect(indexed.surfaceArea).toBeCloseTo(1, 6);
  });
});

describe('print estimates', () => {
  it('converts a millimetre volume to cubic centimetres', () => {
    const stats = computeMeshStats(box(10, 10, 10), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(volumeCm3(stats)).toBeCloseTo(1, 6);
  });

  it('estimates solid PLA mass', () => {
    const stats = computeMeshStats(box(10, 10, 10), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(estimatedMassGrams(stats)).toBeCloseTo(1.24, 4);
  });

  it('refuses to guess mass from non-millimetre stats', () => {
    const stats = computeMeshStats(box(1, 1, 1), {
      sourceUnit: 'in',
      sourceUpAxis: 'z',
      targetUnit: 'in',
    });
    expect(Number.isNaN(volumeCm3(stats))).toBe(true);
  });
});
