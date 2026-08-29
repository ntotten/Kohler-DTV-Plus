import { describe, expect, it } from 'vitest';
import {
  ASPECT_TOLERANCE,
  DecalError,
  decalQuad,
  decalsFor,
  MAX_LIFT_MM,
  rasterSize,
  summarizeDecal,
  validateDecal,
  validateDecalSet,
  type DecalRecord,
} from './decals';
import { applyMatrixElements, exportToDisplayMatrixElements, displayMatrixElements } from './units';

// The K-99693 faceplate, measured off the mesh: a single flat quad at
// Y = 15.2734 mm spanning X -65.5273..65.5418 and Z -40.7398..40.7279.
//
// The part is PORTRAIT — the K-99694 bracket drawing gives it as 84 x 143 mm
// with the wiring boss at the bottom — so the product's vertical runs along the
// CAD's X axis, product-down at +X. A viewer standing in front looks along -Y
// with up at -X, which puts their right hand on -Z. Hence u along -Z and v
// along -X, which is what makes `u x v` come out as +Y.
const FACEPLATE: DecalRecord = {
  decalId: 'test-faceplate',
  title: 'Test faceplate',
  sourceModelId: 'kohler-dtv-plus/k-99693/obj',
  space: 'export-mm-zup',
  anchor: {
    origin: [65.5418, 15.2734, 40.7279],
    u: [0, 0, -81.4677],
    v: [-131.0691, 0, 0],
  },
  image: { url: 'decals/test.svg', intrinsicWidth: 814.677, intrinsicHeight: 1310.691 },
  liftMm: 0.12,
  provenanceNote: 'Synthetic fixture.',
};

const clone = (over: Partial<DecalRecord>): DecalRecord => ({ ...FACEPLATE, ...over });

describe('anchor geometry', () => {
  it('derives width, height and outward normal from u and v alone', () => {
    const quad = decalQuad(FACEPLATE);
    expect(quad.widthMm).toBeCloseTo(81.4677, 6);
    expect(quad.heightMm).toBeCloseTo(131.0691, 6);
    // u x v must point out of the faceplate, toward whoever is looking at it.
    expect(quad.normal[0]).toBeCloseTo(0, 12);
    expect(quad.normal[1]).toBeCloseTo(1, 12);
    expect(quad.normal[2]).toBeCloseTo(0, 12);
  });

  it('is portrait, matching the K-99694 bracket drawing', () => {
    // 84 x 143 mm bracket, wiring boss at the bottom. If this ever comes out
    // landscape again, the anchor has been rotated back onto the CAD's own
    // axes and the artwork will be sideways on the part.
    const quad = decalQuad(FACEPLATE);
    expect(quad.anchorAspect).toBeLessThan(1);
    expect(quad.anchorAspect).toBeCloseTo(0.6216, 4);
  });

  it('flips the normal into the part when u and v are swapped', () => {
    // The whole point of deriving the normal rather than declaring it: getting
    // the edges the wrong way round produces a visible, checkable symptom
    // instead of artwork that is silently mirrored.
    const swapped = decalQuad(
      clone({ anchor: { ...FACEPLATE.anchor, u: FACEPLATE.anchor.v, v: FACEPLATE.anchor.u } }),
    );
    expect(swapped.normal[1]).toBeCloseTo(-1, 12);
  });

  it('lifts every corner off the face by exactly liftMm, along the normal', () => {
    const quad = decalQuad(FACEPLATE);
    for (const corner of quad.corners) {
      expect(corner[1]).toBeCloseTo(15.2734 + 0.12, 9);
    }
  });

  it('lays the corners out counter-clockwise from the image origin', () => {
    const [bl, br, tr, tl] = decalQuad(clone({ liftMm: 0 })).corners;
    // Image bottom-left is the part's bottom-left as installed: the +X end of
    // the CAD (product-down) and the +Z side (the viewer's left).
    expect(bl).toEqual([65.5418, 15.2734, 40.7279]);
    // Across the image: Z decreases, X unchanged.
    expect(br[0]).toBeCloseTo(65.5418, 6);
    expect(br[2]).toBeCloseTo(-40.7398, 6);
    // Up the image: X decreases toward the top of the part.
    expect(tr[0]).toBeCloseTo(-65.5273, 6);
    expect(tr[2]).toBeCloseTo(-40.7398, 6);
    expect(tl[0]).toBeCloseTo(-65.5273, 6);
    expect(tl[2]).toBeCloseTo(40.7279, 6);
  });

  it('pairs UVs with corners so the image origin lands on the anchor origin', () => {
    expect(decalQuad(FACEPLATE).uvs).toEqual([
      [0, 0],
      [1, 0],
      [1, 1],
      [0, 1],
    ]);
  });
});

describe('fit', () => {
  it('fills the anchor when the aspects agree', () => {
    const quad = decalQuad(FACEPLATE);
    expect(quad.letterboxed).toBe(false);
    expect(quad.widthMm).toBeCloseTo(quad.anchorWidthMm, 9);
    expect(quad.heightMm).toBeCloseTo(quad.anchorHeightMm, 9);
  });

  it('takes the app UI screenshot at its own aspect, unstretched', () => {
    // The app's UI is 1120x1800 (0.6222) and the faceplate is 0.6216 — 0.11%
    // apart, because the UI was laid out to the proportions of the real device.
    // If either drifts, this is the test that notices.
    const quad = decalQuad(
      clone({ image: { url: 'decals/shot.png', intrinsicWidth: 1120, intrinsicHeight: 1800 } }),
    );
    expect(quad.letterboxed).toBe(false);
    expect(Math.abs(quad.anchorAspect / quad.imageAspect - 1)).toBeLessThan(0.002);
  });

  it('letterboxes a landscape image onto the portrait face without distorting it', () => {
    const quad = decalQuad(
      clone({
        fit: 'contain',
        image: { url: 'decals/wide.png', intrinsicWidth: 1800, intrinsicHeight: 1120 },
      }),
    );
    expect(quad.letterboxed).toBe(true);
    expect(quad.widthMm).toBeCloseTo(81.4677, 6);
    expect(quad.widthMm / quad.heightMm).toBeCloseTo(1800 / 1120, 9);
    expect(quad.heightMm).toBeLessThan(quad.anchorHeightMm);
  });

  it('centres a letterboxed quad inside its anchor', () => {
    const quad = decalQuad(
      clone({
        fit: 'contain',
        liftMm: 0,
        image: { url: 'decals/wide.png', intrinsicWidth: 1800, intrinsicHeight: 1120 },
      }),
    );
    // The anchor's centre in X is (65.5418 + -65.5273) / 2; the quad's centre
    // must match it, otherwise "contain" has quietly become "align to a corner".
    const centreX = (quad.corners[0][0] + quad.corners[3][0]) / 2;
    expect(centreX).toBeCloseTo((65.5418 - 65.5273) / 2, 9);
  });
});

describe('validation', () => {
  it('accepts the measured faceplate anchor', () => {
    expect(() => validateDecal(FACEPLATE)).not.toThrow();
  });

  it('rejects a stretched aspect mismatch by name and by number', () => {
    // Squashed artwork renders perfectly and is wrong — the same failure mode
    // as a guessed unit, so it gets the same treatment: refuse, loudly. The
    // case that matters here is landscape artwork on the portrait faceplate.
    const sideways = { url: 'x.png', intrinsicWidth: 1800, intrinsicHeight: 1120 };
    expect(() => validateDecal(clone({ image: sideways }))).toThrow(DecalError);
    expect(() => validateDecal(clone({ image: sideways }))).toThrow(/fit.*contain/s);
  });

  it('allows the same mismatch once contain is declared', () => {
    expect(() =>
      validateDecal(
        clone({
          fit: 'contain',
          image: { url: 'x.png', intrinsicWidth: 1800, intrinsicHeight: 1120 },
        }),
      ),
    ).not.toThrow();
  });

  it('tolerates aspect drift up to the stated tolerance and no further', () => {
    const w = 81.4677;
    const h = 131.0691;
    const inside = w * (1 + ASPECT_TOLERANCE * 0.9);
    const outside = w * (1 + ASPECT_TOLERANCE * 1.5);
    expect(() =>
      validateDecal(clone({ image: { url: 'x.svg', intrinsicWidth: inside, intrinsicHeight: h } })),
    ).not.toThrow();
    expect(() =>
      validateDecal(
        clone({ image: { url: 'x.svg', intrinsicWidth: outside, intrinsicHeight: h } }),
      ),
    ).toThrow(DecalError);
  });

  it('rejects a sheared anchor', () => {
    expect(() =>
      validateDecal(clone({ anchor: { ...FACEPLATE.anchor, v: [10, 0, 81.4677] } })),
    ).toThrow(/perpendicular/);
  });

  it('rejects an unstated or foreign coordinate space', () => {
    expect(() => validateDecal(clone({ space: undefined as never }))).toThrow(/export-mm-zup/);
    expect(() => validateDecal(clone({ space: 'display' as never }))).toThrow(/export-mm-zup/);
  });

  it('requires a provenance note', () => {
    expect(() => validateDecal(clone({ provenanceNote: '' }))).toThrow(/provenanceNote/);
  });

  it('rejects a lift big enough to detach the decal from the part', () => {
    expect(() => validateDecal(clone({ liftMm: MAX_LIFT_MM + 0.1 }))).toThrow(/liftMm/);
    expect(() => validateDecal(clone({ liftMm: -0.1 }))).toThrow(/liftMm/);
  });

  it('rejects a degenerate anchor', () => {
    expect(() => validateDecal(clone({ anchor: { ...FACEPLATE.anchor, u: [0, 0, 0] } }))).toThrow(
      /length/,
    );
  });

  it('rejects duplicate ids in a set', () => {
    expect(() => validateDecalSet({ decalsVersion: '1', decals: [FACEPLATE, clone({})] })).toThrow(
      /duplicate/,
    );
  });
});

describe('rendering helpers', () => {
  it('sizes a raster from real millimetres, not from the source artwork', () => {
    const quad = decalQuad(FACEPLATE);
    expect(rasterSize(clone({ renderPxPerMm: 12 }), quad)).toEqual({ width: 978, height: 1573 });
  });

  it('summarises a decal in millimetres and normal direction', () => {
    const quad = decalQuad(FACEPLATE);
    expect(summarizeDecal(FACEPLATE, quad)).toContain('81.47 x 131.07 mm');
    expect(summarizeDecal(FACEPLATE, quad)).toContain('0.12 mm proud');
  });

  it('selects decals by the model file they are pinned to', () => {
    const set = {
      decalsVersion: '1',
      decals: [FACEPLATE, clone({ decalId: 'other', sourceModelId: 'other/part/obj' })],
    };
    expect(decalsFor(set, 'kohler-dtv-plus/k-99693/obj').map((d) => d.decalId)).toEqual([
      'test-faceplate',
    ]);
  });
});

describe('export-to-display transform', () => {
  it('maps export mm Z-up onto display mm Y-up', () => {
    expect(applyMatrixElements(exportToDisplayMatrixElements(), 1, 2, 3)).toEqual([1, 3, -2]);
  });

  it('composes with the source-to-export step to equal the display matrix', () => {
    // Decals are authored in export space and drawn in display space. If these
    // two paths ever drift, decals and geometry stop agreeing on where the part
    // is — and the symptom is artwork that looks fine from one angle only.
    const source: Array<[number, number, number]> = [
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1],
      [2.58, 0.6, -1.6],
    ];
    for (const upAxis of ['x', 'y', 'z'] as const) {
      const direct = displayMatrixElements(upAxis, 'in');
      for (const [x, y, z] of source) {
        const viaExport = applyMatrixElements(
          exportToDisplayMatrixElements(),
          ...exportPoint(upAxis, x, y, z),
        );
        const expected = applyMatrixElements(direct, x, y, z);
        for (let i = 0; i < 3; i++) expect(viaExport[i]).toBeCloseTo(expected[i], 9);
      }
    }
  });
});

/** Source -> export space: scale to mm, rotate the declared up axis onto +Z. */
function exportPoint(
  upAxis: 'x' | 'y' | 'z',
  x: number,
  y: number,
  z: number,
): [number, number, number] {
  const s = 25.4;
  switch (upAxis) {
    case 'z':
      return [x * s, y * s, z * s];
    case 'y':
      return [x * s, -z * s, y * s];
    case 'x':
      return [-z * s, y * s, x * s];
  }
}
