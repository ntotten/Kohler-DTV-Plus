import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import * as THREE from 'three';
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js';
import { STLLoader } from 'three/addons/loaders/STLLoader.js';
import catalogJson from '../src/catalog/catalog.json';
import decalsJson from '../src/catalog/decals.json';
import { flattenCatalog, validateCatalog } from '../src/catalog/catalog';
import { decalQuad, decalsFor, summarizeDecal, validateDecalSet } from '../src/core/decals';
import { extractSourceGeometry } from '../src/scene/loaders';
import { computeMeshStats, estimatedMassGrams, volumeCm3 } from '../src/core/meshStats';
import { repairMesh, summarizeRepair } from '../src/core/repair';
import { exportFilename, readBinaryStlCount, writeBinaryStl } from '../src/core/stl';
import type { ModelFormat } from '../src/catalog/types';

// Offline gate: load every vendored catalog asset, export it, and re-read the
// exported bytes to confirm the geometry survived the trip.
//
// The unit tests prove the maths on synthetic boxes. This proves it on the
// actual manufacturer CAD, which is where the real risks live — a mis-declared
// unit, an asset replaced by a newer revision with different dimensions, a
// loader change that silently drops a node transform. Run it in CI and before
// trusting a downloaded file.
//
// Run: npm run verify

const ROOT = resolve(import.meta.dirname, '..');

/** Formats this gate can load without a browser. Others are reported skipped. */
const OFFLINE_FORMATS: ReadonlySet<ModelFormat> = new Set<ModelFormat>(['obj', 'stl']);

interface Failure {
  where: string;
  message: string;
}

const failures: Failure[] = [];

function check(where: string, condition: boolean, message: string): void {
  if (!condition) failures.push({ where, message });
}

const catalog = validateCatalog(catalogJson);
const entries = flattenCatalog(catalog);
const decalSet = validateDecalSet(decalsJson);
console.log(
  `catalog ${catalog.catalogVersion}: ${entries.length} part(s), ` +
    `decals ${decalSet.decalsVersion}: ${decalSet.decals.length}\n`,
);

for (const entry of entries) {
  const where = `${entry.family.familyId}/${entry.part.partId}`;
  const { file } = entry;

  if (!OFFLINE_FORMATS.has(file.format)) {
    console.log(`- ${where}: SKIPPED (${file.format} needs a browser to parse)\n`);
    continue;
  }

  const path = resolve(ROOT, 'public', file.url);
  let raw: Buffer;
  try {
    raw = readFileSync(path);
  } catch {
    // A missing vendored asset is a warning, not a failure: PROVENANCE.md
    // explicitly allows the manufacturer CAD to be absent, and the app still
    // works via drag-and-drop.
    console.log(`- ${where}: asset not present at ${file.url} — skipping\n`);
    continue;
  }

  const object =
    file.format === 'obj'
      ? new OBJLoader().parse(raw.toString('utf8'))
      : new THREE.Mesh(new STLLoader().parse(bufferOf(raw)));

  const geometry = extractSourceGeometry(object);
  const stats = computeMeshStats(geometry, {
    sourceUnit: file.sourceUnit,
    sourceUpAxis: file.sourceUpAxis,
  });

  const result = writeBinaryStl(geometry, {
    sourceUnit: file.sourceUnit,
    sourceUpAxis: file.sourceUpAxis,
    targetUnit: 'mm',
  });

  console.log(`- ${where} (${file.name})`);
  console.log(`    source        ${file.sourceUnit}, ${file.sourceUpAxis}-up`);
  console.log(`    triangles     ${stats.triangleCount.toLocaleString()}`);
  console.log(`    envelope      ${stats.size.map((n) => n.toFixed(2)).join(' x ')} mm`);
  console.log(`    surface       ${(stats.surfaceArea / 100).toFixed(2)} cm2`);
  console.log(
    stats.closed
      ? `    volume        ${volumeCm3(stats).toFixed(2)} cm3  (${estimatedMassGrams(stats).toFixed(1)} g solid PLA)`
      : `    mesh          OPEN — ${stats.boundaryEdges.toLocaleString()} unshared edges, volume withheld`,
  );
  console.log(
    `    export        ${exportFilename(entry.part.sku ?? entry.part.title, 'mm')} (${result.buffer.byteLength.toLocaleString()} bytes)`,
  );

  // Structural read-back.
  let written = 0;
  try {
    written = readBinaryStlCount(result.buffer);
  } catch (error) {
    failures.push({ where, message: `exported STL failed its own length check: ${String(error)}` });
  }
  check(
    where,
    written === stats.triangleCount,
    `wrote ${written} triangles, expected ${stats.triangleCount}`,
  );

  // Geometric read-back: re-derive the envelope from the exported bytes rather
  // than trusting the in-memory numbers. This is what catches a transform that
  // was applied for measurement but not for export, or vice versa.
  const reread = envelopeOf(result.buffer);
  for (let axis = 0; axis < 3; axis++) {
    const delta = Math.abs(reread.size[axis] - stats.size[axis]);
    check(
      where,
      delta < 0.01,
      `axis ${'XYZ'[axis]}: exported envelope ${reread.size[axis].toFixed(3)} mm ` +
        `disagrees with measured ${stats.size[axis].toFixed(3)} mm`,
    );
  }
  console.log(
    `    read-back     ${reread.size.map((n) => n.toFixed(2)).join(' x ')} mm, ${reread.degenerate} degenerate facet(s)`,
  );

  // A part whose largest dimension lands under 1 mm or over 2 m is almost
  // certainly a units error rather than a real part, and that is exactly the
  // failure this whole app exists to prevent.
  const largest = Math.max(...stats.size);
  check(
    where,
    largest > 1 && largest < 2000,
    `largest dimension ${largest.toFixed(2)} mm looks like a units error`,
  );

  // ---- repair path ----------------------------------------------------------
  const repair = repairMesh(geometry, file.sourceUnit);
  const repaired = computeMeshStats(repair.geometry, {
    sourceUnit: file.sourceUnit,
    sourceUpAxis: file.sourceUpAxis,
  });

  console.log(`    repair        ${summarizeRepair(repair.report)}`);
  console.log(
    `                  ${repair.report.trianglesBefore} -> ${repaired.triangleCount} triangles, ` +
      `boundary ${repair.report.boundaryEdgesBefore} -> ${repair.report.boundaryEdgesAfter}, ` +
      `non-manifold ${repair.report.nonManifoldEdges}`,
  );
  if (repaired.closed) {
    console.log(`    enclosed vol  ${volumeCm3(repaired).toFixed(2)} cm3`);
  }

  // THE critical repair invariant. Capping a hole adds interior geometry; it
  // must never move the outer surface, because that surface is what a toolpath
  // is cut against. A drift here means the repair is dimensionally lying.
  for (let axis = 0; axis < 3; axis++) {
    const drift = Math.abs(repaired.size[axis] - stats.size[axis]);
    check(
      where,
      drift < 1e-4,
      `repair moved the envelope on ${'XYZ'[axis]} by ${drift.toFixed(6)} mm — ` +
        `the repaired mesh is not dimensionally equivalent to the source`,
    );
  }

  // Repair must never make topology worse than it found it.
  check(
    where,
    repair.report.boundaryEdgesAfter <= repair.report.boundaryEdgesBefore,
    `repair increased boundary edges from ${repair.report.boundaryEdgesBefore} to ${repair.report.boundaryEdgesAfter}`,
  );

  // ---- decals ---------------------------------------------------------------
  //
  // A decal is authored by reading coordinates off the model, which means a
  // decal can be wrong in a way nothing else here can: the numbers can be
  // internally consistent and simply not be on the part. So the anchor is
  // checked against the exported triangles — the same bytes a machine would
  // get — rather than against the record that produced it.
  const modelId = `${entry.family.familyId}/${entry.part.partId}/${file.id}`;
  const decals = decalsFor(decalSet, modelId);
  if (decals.length) {
    const surface = trianglesOf(result.buffer);
    console.log(`    decals        ${decals.length} pinned to this file`);

    for (const decal of decals) {
      const quad = decalQuad(decal);
      const lift = decal.liftMm ?? 0;
      const imagePath = resolve(ROOT, 'public', decal.image.url);
      let imageBytes = 0;
      try {
        imageBytes = readFileSync(imagePath).byteLength;
      } catch {
        failures.push({
          where,
          message: `decal ${decal.decalId}: artwork missing at ${decal.image.url}`,
        });
      }

      // Every corner must sit on the surface, allowing for the lift it was
      // asked to float by. A decal drifting off the part — a mistyped sign, a
      // face that moved when the asset was revised — fails here rather than
      // being noticed as "looks a bit odd" months later.
      let worst = 0;
      for (const corner of quad.corners) {
        worst = Math.max(worst, distanceToSurface(surface, corner) - lift);
      }
      const tolerance = 0.05;
      check(
        where,
        worst <= tolerance,
        `decal ${decal.decalId}: a corner sits ${worst.toFixed(3)} mm off the mesh ` +
          `(lift ${lift.toFixed(2)} mm, tolerance ${tolerance} mm) — the anchor is not on the part`,
      );

      // The normal is derived from u x v, so a swapped pair of edges faces the
      // artwork into the part. Distance alone cannot see that — a shell is the
      // same distance away from either side — so the decal's normal is compared
      // against the outward normal of the facet it is actually sitting on.
      const centre: [number, number, number] = [
        (quad.corners[0][0] + quad.corners[2][0]) / 2,
        (quad.corners[0][1] + quad.corners[2][1]) / 2,
        (quad.corners[0][2] + quad.corners[2][2]) / 2,
      ];
      const agreement = facingAgreement(surface, centre, quad.normal);
      check(
        where,
        agreement > 0.9,
        `decal ${decal.decalId}: its normal agrees with the face beneath it by only ` +
          `${agreement.toFixed(3)} — u and v are probably swapped, which faces the ` +
          `artwork into the part`,
      );

      console.log(`      ${decal.decalId}`);
      console.log(`        anchor      ${summarizeDecal(decal, quad)}`);
      console.log(
        `        aspect      face ${quad.anchorAspect.toFixed(4)}, artwork ${quad.imageAspect.toFixed(4)} (${decal.fit ?? 'stretch'})`,
      );
      console.log(`        surface     worst corner ${worst.toFixed(4)} mm off the mesh`);
      console.log(`        artwork     ${decal.image.url} (${imageBytes.toLocaleString()} bytes)`);
    }

    // The guarantee the whole design rests on. Decals are drawn from their own
    // scene root; the exporter reads a geometry snapshot taken at load. If a
    // decal could ever add a triangle, this is where it would show.
    check(
      where,
      written === stats.triangleCount,
      `${decals.length} decal(s) present but the export triangle count moved — ` +
        `decals must never reach the exported geometry`,
    );
  }

  for (const skip of repair.report.skipped) {
    console.log(
      `    ! skipped     ${skip.points} pts, ${skip.perimeterMm.toFixed(1)} mm perimeter, ` +
        `${skip.planarityMm.toFixed(3)} mm out of plane — ${skip.reason}`,
    );
  }
  console.log('');
}

if (failures.length) {
  console.error(`FAILED — ${failures.length} problem(s):`);
  for (const failure of failures) console.error(`  ${failure.where}: ${failure.message}`);
  process.exit(1);
}
console.log('OK — every catalog asset exported and read back consistently.');

// ---------------------------------------------------------------- helpers

function bufferOf(raw: Buffer): ArrayBuffer {
  return raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength) as ArrayBuffer;
}

/** Every triangle of an exported binary STL, flattened: 9 floats per facet. */
function trianglesOf(buffer: ArrayBuffer): Float64Array {
  const view = new DataView(buffer);
  const count = view.getUint32(80, true);
  const out = new Float64Array(count * 9);
  for (let t = 0; t < count; t++) {
    const base = 84 + t * 50 + 12;
    for (let i = 0; i < 9; i++) out[t * 9 + i] = view.getFloat32(base + i * 4, true);
  }
  return out;
}

/**
 * Shortest distance from a point to the closest point on any triangle.
 *
 * Brute force over every facet. At a few thousand triangles and a handful of
 * decal corners that is a few tens of milliseconds, and an exact answer beats
 * an acceleration structure that could itself be the thing that is wrong.
 */
function distanceToSurface(triangles: Float64Array, p: readonly number[]): number {
  let best = Infinity;
  for (let i = 0; i < triangles.length; i += 9) {
    const d = pointTriangleDistance(p, triangles, i);
    if (d < best) best = d;
  }
  return best;
}

/**
 * `dot` between a decal's normal and the outward normal of the mesh facet
 * closest to it. 1 means the decal faces the same way as the surface it is
 * mounted on; -1 means it is buried in the part, facing inwards.
 *
 * The facet normal is recomputed from the winding rather than read from the
 * STL's stored normal, which some writers leave zeroed.
 */
function facingAgreement(
  triangles: Float64Array,
  point: readonly number[],
  normal: readonly number[],
): number {
  let best = Infinity;
  let at = 0;
  for (let i = 0; i < triangles.length; i += 9) {
    const d = pointTriangleDistance(point, triangles, i);
    if (d < best) {
      best = d;
      at = i;
    }
  }
  const abx = triangles[at + 3] - triangles[at];
  const aby = triangles[at + 4] - triangles[at + 1];
  const abz = triangles[at + 5] - triangles[at + 2];
  const acx = triangles[at + 6] - triangles[at];
  const acy = triangles[at + 7] - triangles[at + 1];
  const acz = triangles[at + 8] - triangles[at + 2];
  const nx = aby * acz - abz * acy;
  const ny = abz * acx - abx * acz;
  const nz = abx * acy - aby * acx;
  const l = Math.hypot(nx, ny, nz) || 1;
  return (normal[0] * nx + normal[1] * ny + normal[2] * nz) / l;
}

function pointTriangleDistance(p: readonly number[], t: Float64Array, o: number): number {
  const ax = t[o],
    ay = t[o + 1],
    az = t[o + 2];
  const abx = t[o + 3] - ax,
    aby = t[o + 4] - ay,
    abz = t[o + 5] - az;
  const acx = t[o + 6] - ax,
    acy = t[o + 7] - ay,
    acz = t[o + 8] - az;
  const apx = p[0] - ax,
    apy = p[1] - ay,
    apz = p[2] - az;

  // Barycentric projection onto the triangle plane, clamped to the triangle —
  // the standard Ericson formulation.
  const d1 = abx * apx + aby * apy + abz * apz;
  const d2 = acx * apx + acy * apy + acz * apz;
  let u = 0;
  let v = 0;
  if (d1 > 0 || d2 > 0) {
    const bpx = p[0] - (ax + abx),
      bpy = p[1] - (ay + aby),
      bpz = p[2] - (az + abz);
    const d3 = abx * bpx + aby * bpy + abz * bpz;
    const d4 = acx * bpx + acy * bpy + acz * bpz;
    const cpx = p[0] - (ax + acx),
      cpy = p[1] - (ay + acy),
      cpz = p[2] - (az + acz);
    const d5 = abx * cpx + aby * cpy + abz * cpz;
    const d6 = acx * cpx + acy * cpy + acz * cpz;
    const vc = d1 * d4 - d3 * d2;
    const vb = d5 * d2 - d1 * d6;
    const va = d3 * d6 - d5 * d4;

    if (vc <= 0 && d1 >= 0 && d3 <= 0) {
      u = d1 / (d1 - d3);
    } else if (vb <= 0 && d2 >= 0 && d6 <= 0) {
      v = d2 / (d2 - d6);
    } else if (va <= 0 && d4 - d3 >= 0 && d5 - d6 >= 0) {
      const w = (d4 - d3) / (d4 - d3 + (d5 - d6));
      u = 1 - w;
      v = w;
    } else if (d3 >= 0 && d4 <= 0) {
      u = 1;
    } else if (d6 >= 0 && d5 <= 0) {
      v = 1;
    } else {
      const denom = 1 / (va + vb + vc);
      u = vb * denom;
      v = vc * denom;
    }
  }

  const cx = ax + abx * u + acx * v;
  const cy = ay + aby * u + acy * v;
  const cz = az + abz * u + acz * v;
  return Math.hypot(p[0] - cx, p[1] - cy, p[2] - cz);
}

function envelopeOf(buffer: ArrayBuffer): { size: number[]; degenerate: number } {
  const view = new DataView(buffer);
  const count = view.getUint32(80, true);
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  let degenerate = 0;

  for (let t = 0; t < count; t++) {
    const base = 84 + t * 50;
    const nx = view.getFloat32(base, true);
    const ny = view.getFloat32(base + 4, true);
    const nz = view.getFloat32(base + 8, true);
    if (nx === 0 && ny === 0 && nz === 0) degenerate++;
    for (let corner = 0; corner < 3; corner++) {
      for (let axis = 0; axis < 3; axis++) {
        const value = view.getFloat32(base + 12 + corner * 12 + axis * 4, true);
        if (value < min[axis]) min[axis] = value;
        if (value > max[axis]) max[axis] = value;
      }
    }
  }
  return { size: max.map((m, i) => m - min[i]), degenerate };
}
