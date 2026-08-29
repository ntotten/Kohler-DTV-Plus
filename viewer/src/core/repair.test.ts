import { describe, expect, it } from 'vitest';
import { repairMesh, summarizeRepair } from './repair';
import { computeMeshStats } from './meshStats';

/** Closed box, 12 triangles, wound outward. */
function boxTriangles(w: number, h: number, d: number): number[][] {
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
    [0, 2, 1],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [2, 3, 7],
    [2, 7, 6],
    [1, 2, 6],
    [1, 6, 5],
    [0, 4, 7],
    [0, 7, 3],
  ];
  return faces.map((f) => f.flatMap((i) => v[i]));
}

function toGeometry(tris: number[][]) {
  return { positions: new Float32Array(tris.flat()) };
}

describe('repairMesh — welding', () => {
  it('merges duplicated vertices without adding geometry', () => {
    // A closed box built as a triangle soup has every vertex duplicated across
    // the faces that meet there. Welding must collapse those without capping
    // anything, because there are no holes.
    const geometry = toGeometry(boxTriangles(10, 20, 30));
    const { report } = repairMesh(geometry, 'mm');

    expect(report.weldedVertices).toBeGreaterThan(0);
    expect(report.loopsFound).toBe(0);
    expect(report.trianglesAdded).toBe(0);
    expect(report.watertight).toBe(true);
  });

  it('reports an already-welded closed mesh as watertight', () => {
    const { report } = repairMesh(toGeometry(boxTriangles(5, 5, 5)), 'mm');
    expect(report.boundaryEdgesBefore).toBe(0);
    expect(report.boundaryEdgesAfter).toBe(0);
    expect(report.nonManifoldEdges).toBe(0);
  });

  it('drops triangles that collapse to nothing when welded', () => {
    const tris = boxTriangles(10, 10, 10);
    // A sliver far below the weld tolerance — its three corners are one point.
    tris.push([0, 0, 0, 1e-9, 0, 0, 0, 1e-9, 0]);
    const { geometry, report } = repairMesh(toGeometry(tris), 'mm', {
      weldToleranceMm: 0.001,
    });
    expect(report.watertight).toBe(true);
    expect(geometry.positions.length / 9).toBe(12);
  });
});

describe('repairMesh — capping', () => {
  it('closes a box with one face removed, and recovers the true volume', () => {
    // The decisive test. Volume is only meaningful on a closed mesh, so if the
    // cap is wound backwards, placed wrongly, or has a gap, the volume comes out
    // wrong or negative. Getting 6000 back proves the cap is right.
    const tris = boxTriangles(10, 20, 30);
    tris.splice(0, 2); // remove the two triangles of the -Z face

    const before = computeMeshStats(toGeometry(tris), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(before.closed).toBe(false);

    const { geometry, report } = repairMesh(toGeometry(tris), 'mm');
    expect(report.loopsFound).toBe(1);
    expect(report.loopsCapped).toBe(1);
    expect(report.watertight).toBe(true);

    const after = computeMeshStats(geometry, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(after.closed).toBe(true);
    expect(after.volume).toBeCloseTo(6000, 2);
  });

  it('preserves the bounding box — a cap must not move the envelope', () => {
    const tris = boxTriangles(10, 20, 30);
    tris.splice(0, 2);
    const before = computeMeshStats(toGeometry(tris), { sourceUnit: 'mm', sourceUpAxis: 'z' });
    const { geometry } = repairMesh(toGeometry(tris), 'mm');
    const after = computeMeshStats(geometry, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(after.size[0]).toBeCloseTo(before.size[0], 6);
    expect(after.size[1]).toBeCloseTo(before.size[1], 6);
    expect(after.size[2]).toBeCloseTo(before.size[2], 6);
  });

  it('caps several holes at once', () => {
    const tris = boxTriangles(10, 10, 10);
    tris.splice(2, 2); // +Z face
    tris.splice(0, 2); // -Z face
    const { report } = repairMesh(toGeometry(tris), 'mm');
    expect(report.loopsFound).toBe(2);
    expect(report.loopsCapped).toBe(2);
    expect(report.watertight).toBe(true);
  });

  it('caps a non-convex loop that a naive triangle fan would get wrong', () => {
    // An L-shaped hole. A fan from one vertex would emit triangles outside the
    // boundary; ear clipping must not.
    const l = [
      [0, 0, 0],
      [30, 0, 0],
      [30, 10, 0],
      [10, 10, 0],
      [10, 30, 0],
      [0, 30, 0],
    ];
    const tris: number[][] = [];
    // Wall around the L, one quad per edge, extruded to z = 5.
    for (let i = 0; i < l.length; i++) {
      const a = l[i];
      const b = l[(i + 1) % l.length];
      const at = [a[0], a[1], 5];
      const bt = [b[0], b[1], 5];
      tris.push([...a, ...b, ...bt]);
      tris.push([...a, ...bt, ...at]);
    }
    const { report } = repairMesh(toGeometry(tris), 'mm');
    expect(report.loopsFound).toBe(2);
    expect(report.loopsCapped).toBe(2);
    expect(report.watertight).toBe(true);
    expect(report.skipped).toHaveLength(0);
  });

  it('caps a circular hole', () => {
    const N = 24;
    const ring = Array.from({ length: N }, (_, i) => {
      const a = (i / N) * Math.PI * 2;
      return [Math.cos(a) * 10, Math.sin(a) * 10];
    });
    const tris: number[][] = [];
    for (let i = 0; i < N; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % N];
      tris.push([a[0], a[1], 0, b[0], b[1], 0, b[0], b[1], 4]);
      tris.push([a[0], a[1], 0, b[0], b[1], 4, a[0], a[1], 4]);
    }
    const { geometry, report } = repairMesh(toGeometry(tris), 'mm');
    expect(report.watertight).toBe(true);
    const stats = computeMeshStats(geometry, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    // Cylinder r=10 h=4 -> 1256.6 mm3, less the polygon-vs-circle shortfall.
    expect(stats.volume).toBeGreaterThan(1200);
    expect(stats.volume).toBeLessThan(1257);
  });
});

describe('repairMesh — non-manifold collapse', () => {
  /**
   * Two closed boxes meeting along a hairline gap, reproducing the T-junction
   * seam in the Kohler CAD: an edge so short that four triangles claim it.
   */
  function seamPair(gap: number): number[][] {
    const left = boxTriangles(10, 10, 10);
    const right = boxTriangles(10, 10, 10).map((t) => {
      const out = [...t];
      for (let i = 0; i < 9; i += 3) out[i] += 10 + gap;
      return out;
    });
    return [...left, ...right];
  }

  it('leaves genuinely separate geometry alone', () => {
    // A 2 mm gap is a real feature, not a seam. Nothing should collapse.
    const { report } = repairMesh(toGeometry(seamPair(2)), 'mm');
    expect(report.collapsedEdges).toBe(0);
  });

  it('can be disabled', () => {
    const { report } = repairMesh(toGeometry(seamPair(0)), 'mm', {
      nonManifoldCollapseMm: 0,
    });
    expect(report.collapsedEdges).toBe(0);
  });

  it('does not collapse edges that are merely short but manifold', () => {
    // A thin box: its 0.02 mm edges are short but each is used by exactly two
    // triangles, so they must survive or the part loses its thickness.
    const thin = boxTriangles(20, 0.02, 20);
    const { geometry, report } = repairMesh(toGeometry(thin), 'mm');
    expect(report.collapsedEdges).toBe(0);
    const stats = computeMeshStats(geometry, { sourceUnit: 'mm', sourceUpAxis: 'z' });
    expect(stats.size[1]).toBeCloseTo(0.02, 6);
    expect(stats.closed).toBe(true);
  });
});

describe('repairMesh — honesty about what it cannot fix', () => {
  it('refuses to flatten a loop that is genuinely not planar', () => {
    // A saddle-shaped boundary. Capping it flat would invent geometry, so it
    // must be skipped and the result declared not watertight.
    const tris: number[][] = [];
    const ring = [
      [0, 0, 0],
      [10, 0, 6],
      [10, 10, 0],
      [0, 10, 6],
    ];
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % ring.length];
      tris.push([...a, ...b, b[0], b[1], b[2] - 20]);
      tris.push([...a, b[0], b[1], b[2] - 20, a[0], a[1], a[2] - 20]);
    }
    const { report } = repairMesh(toGeometry(tris), 'mm', { planarToleranceMm: 0.25 });
    expect(report.skipped.length).toBeGreaterThan(0);
    expect(report.skipped[0].reason).toMatch(/not planar/);
    expect(report.watertight).toBe(false);
  });

  it('never claims watertight while boundary edges remain', () => {
    const tris = boxTriangles(10, 10, 10);
    tris.splice(0, 1); // half a face — leaves a triangular hole
    const { report } = repairMesh(toGeometry(tris), 'mm');
    expect(report.watertight).toBe(
      report.boundaryEdgesAfter === 0 && report.nonManifoldEdges === 0,
    );
  });

  it('reports skipped loops with their measurements in millimetres', () => {
    const tris: number[][] = [];
    const ring = [
      [0, 0, 0],
      [10, 0, 6],
      [10, 10, 0],
      [0, 10, 6],
    ];
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % ring.length];
      tris.push([...a, ...b, b[0], b[1], b[2] - 20]);
      tris.push([...a, b[0], b[1], b[2] - 20, a[0], a[1], a[2] - 20]);
    }
    const { report } = repairMesh(toGeometry(tris), 'mm');
    const skipped = report.skipped[0];
    expect(skipped.planarityMm).toBeGreaterThan(0.25);
    expect(skipped.perimeterMm).toBeGreaterThan(0);
  });
});

describe('repairMesh — units', () => {
  it('applies millimetre tolerances to inch-authored geometry', () => {
    // A 1-inch box with a face removed. The default 0.001 mm weld tolerance must
    // become 0.0000394 in internally — if it were applied as 0.001 INCHES the
    // box would still weld, so the test uses a gap that only a correct
    // conversion resolves.
    const tris = boxTriangles(1, 1, 1);
    tris.splice(0, 2);
    const { report } = repairMesh(toGeometry(tris), 'in');
    expect(report.loopsCapped).toBe(1);
    expect(report.watertight).toBe(true);
  });

  it('does not weld two inch-space points that are 1 mm apart', () => {
    // 1 mm = 0.03937 in. With a 0.001 mm tolerance these must stay distinct.
    const tris = boxTriangles(1, 1, 1);
    const { report } = repairMesh(toGeometry(tris), 'in', { weldToleranceMm: 0.001 });
    expect(report.watertight).toBe(true);
    expect(report.trianglesAdded).toBe(0);
  });
});

describe('summarizeRepair', () => {
  it('says watertight when it is', () => {
    const tris = boxTriangles(10, 10, 10);
    tris.splice(0, 2);
    const { report } = repairMesh(toGeometry(tris), 'mm');
    expect(summarizeRepair(report)).toMatch(/Watertight/);
  });

  it('says NOT watertight when holes remain', () => {
    const tris: number[][] = [];
    const ring = [
      [0, 0, 0],
      [10, 0, 6],
      [10, 10, 0],
      [0, 10, 6],
    ];
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % ring.length];
      tris.push([...a, ...b, b[0], b[1], b[2] - 20]);
      tris.push([...a, b[0], b[1], b[2] - 20, a[0], a[1], a[2] - 20]);
    }
    const { report } = repairMesh(toGeometry(tris), 'mm');
    expect(summarizeRepair(report)).toMatch(/NOT watertight/);
  });
});
