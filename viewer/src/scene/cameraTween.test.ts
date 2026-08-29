import * as THREE from 'three';
import { describe, expect, it } from 'vitest';
import { blendPose, type CameraPose } from './cameraFit';
import { createCameraTween, ease } from './cameraTween';

// The animation's maths, without a WebGL context. Both halves fail quietly:
// a wrong easing still animates, and a wrong blend still arrives at the right
// place — it just takes a bad path to get there, which no assertion about the
// endpoints would ever catch.

const pose = (
  position: [number, number, number],
  target: [number, number, number] = [0, 0, 0],
  up: [number, number, number] = [0, 1, 0],
): CameraPose => ({
  position: new THREE.Vector3(...position),
  target: new THREE.Vector3(...target),
  up: new THREE.Vector3(...up),
});

describe('ease', () => {
  it('pins both ends exactly, whatever the exponents', () => {
    for (const [i, o] of [
      [1, 1],
      [1.35, 2.6],
      [4, 0.5],
      [0.2, 9],
    ]) {
      expect(ease(0, i, o)).toBe(0);
      expect(ease(1, i, o)).toBe(1);
    }
  });

  it('clamps outside the unit interval', () => {
    expect(ease(-1, 1.35, 2.6)).toBe(0);
    expect(ease(2, 1.35, 2.6)).toBe(1);
    expect(ease(Number.NaN, 1.35, 2.6)).toBe(0);
  });

  it('is the identity when both exponents are 1', () => {
    for (const t of [0.1, 0.25, 0.5, 0.75, 0.9]) {
      expect(ease(t, 1, 1)).toBeCloseTo(t, 12);
    }
  });

  it('is monotonic', () => {
    let previous = -1;
    for (let i = 0; i <= 100; i++) {
      const value = ease(i / 100, 1.35, 2.6);
      expect(value).toBeGreaterThanOrEqual(previous);
      previous = value;
    }
  });

  it('with ease-out the stronger end, is past halfway at halfway', () => {
    // The property that makes the move feel responsive rather than laggy: the
    // bulk of the distance is behind it when half the clock has run, leaving
    // the back half to land softly.
    expect(ease(0.5, 1.35, 2.6)).toBeGreaterThan(0.5);
    expect(ease(0.75, 1.35, 2.6)).toBeGreaterThan(0.9);
  });

  it('still eases in: the first instants are slower than linear', () => {
    // An ease-in above 1 has to start gently — that is what it means — so the
    // curve dips below linear before it crosses above. Asserted so nobody
    // "fixes" the mild ease-in by reading the dip as a bug.
    expect(ease(0.1, 1.35, 2.6)).toBeLessThan(0.1);
    expect(ease(0.25, 1.35, 2.6)).toBeLessThan(0.25);
    // ...and crosses linear somewhere before the midpoint, once and for all.
    const crossings = Array.from({ length: 999 }, (_, i) => (i + 1) / 1000)
      .map((t) => ease(t, 1.35, 2.6) > t)
      .reduce((count, above, i, all) => (i > 0 && above !== all[i - 1] ? count + 1 : count), 0);
    expect(crossings).toBe(1);
  });

  it('reverses that when ease-in is the stronger end', () => {
    expect(ease(0.5, 2.6, 1.35)).toBeLessThan(0.5);
  });

  it('is symmetric about the midpoint when the exponents match', () => {
    for (const t of [0.1, 0.3, 0.45]) {
      expect(ease(t, 2, 2) + ease(1 - t, 2, 2)).toBeCloseTo(1, 12);
    }
  });
});

describe('blendPose', () => {
  const near = (v: THREE.Vector3, x: number, y: number, z: number) => {
    expect(v.x).toBeCloseTo(x, 5);
    expect(v.y).toBeCloseTo(y, 5);
    expect(v.z).toBeCloseTo(z, 5);
  };

  it('returns the endpoints at t=0 and t=1', () => {
    const from = pose([0, 0, 10]);
    const to = pose([10, 0, 0]);
    near(blendPose(from, to, 0).position, 0, 0, 10);
    near(blendPose(from, to, 1).position, 10, 0, 0);
  });

  it('orbits rather than cutting the corner', () => {
    // A straight lerp from +Z to +X passes within 7.07 of the target; an orbit
    // holds the radius. This is the whole reason the blend is not a lerp.
    const blended = blendPose(pose([0, 0, 10]), pose([10, 0, 0]), 0.5);
    expect(blended.position.length()).toBeCloseTo(10, 5);
    near(blended.position, 7.0710678, 0, 7.0710678);
  });

  it('never passes through the target on a 180-degree turn', () => {
    // The worst case for a lerp: it would go straight through the model.
    const from = pose([0, 0, 10]);
    const to = pose([0, 0, -10]);
    for (let i = 0; i <= 20; i++) {
      const blended = blendPose(from, to, i / 20);
      expect(blended.position.distanceTo(blended.target)).toBeGreaterThan(9.99);
    }
  });

  it('interpolates the distance as well as the direction', () => {
    const blended = blendPose(pose([0, 0, 10]), pose([20, 0, 0]), 0.5);
    expect(blended.position.length()).toBeCloseTo(15, 5);
  });

  it('moves the target with the camera', () => {
    const blended = blendPose(pose([0, 0, 10], [0, 0, 0]), pose([0, 0, 14], [0, 0, 4]), 0.5);
    near(blended.target, 0, 0, 2);
  });

  it('carries roll through, keeping up square to the line of sight', () => {
    // From +Z with Y up, to +Z rolled 90 degrees. The camera must not move.
    const from = pose([0, 0, 10], [0, 0, 0], [0, 1, 0]);
    const to = pose([0, 0, 10], [0, 0, 0], [1, 0, 0]);
    for (let i = 0; i <= 10; i++) {
      const blended = blendPose(from, to, i / 10);
      near(blended.position, 0, 0, 10);
      const sight = blended.position.clone().sub(blended.target).normalize();
      expect(blended.up.dot(sight)).toBeCloseTo(0, 6);
      expect(blended.up.length()).toBeCloseTo(1, 6);
    }
    near(blendPose(from, to, 1).up, 1, 0, 0);
  });

  it('keeps up a unit vector square to the sight line all the way round', () => {
    // Swinging up to a near-overhead view, where a naive up would collapse.
    const from = pose([0, 0, 10]);
    const to = pose([0, 10, 0], [0, 0, 0], [0, 0, -1]);
    for (let i = 0; i <= 20; i++) {
      const blended = blendPose(from, to, i / 20);
      const sight = blended.position.clone().sub(blended.target).normalize();
      expect(Math.abs(blended.up.dot(sight))).toBeLessThan(1e-6);
      expect(blended.up.length()).toBeCloseTo(1, 6);
    }
  });
});

describe('createCameraTween', () => {
  const from = pose([0, 0, 10]);
  const to = pose([10, 0, 0]);

  it('runs from start to destination over the duration', () => {
    const tween = createCameraTween({ durationMs: 100, easeIn: 1, easeOut: 1 });
    tween.start(from, to, 1000);
    expect(tween.active).toBe(true);

    const half = tween.sample(1050);
    expect(half?.position.length()).toBeCloseTo(10, 5);
    expect(half?.position.x).toBeGreaterThan(0);
    expect(half?.position.z).toBeLessThan(10);

    const end = tween.sample(1100);
    expect(end?.position.x).toBeCloseTo(10, 5);
    expect(tween.active).toBe(false);
    expect(tween.sample(1200)).toBeNull();
  });

  it('lands on the destination pose exactly, not on a blend at t=1', () => {
    const tween = createCameraTween({ durationMs: 100 });
    tween.start(from, to, 0);
    expect(tween.sample(500)).toBe(to);
  });

  it('with zero duration, delivers the destination on the next frame', () => {
    // How reduced motion turns the animation off — no second code path.
    const tween = createCameraTween({ durationMs: 0 });
    tween.start(from, to, 0);
    expect(tween.sample(0)).toBe(to);
    expect(tween.active).toBe(false);
  });

  it('restarts from where it is when interrupted', () => {
    const tween = createCameraTween({ durationMs: 100, easeIn: 1, easeOut: 1 });
    tween.start(from, to, 0);
    const midway = tween.sample(50);
    expect(midway).not.toBeNull();

    const third = pose([0, 10, 0]);
    tween.start(midway!, third, 50);
    // The first frame of the new move must continue from the interrupted
    // pose, not jump back to where the first move began.
    const next = tween.sample(50);
    expect(next?.position.distanceTo(midway!.position)).toBeCloseTo(0, 5);
  });

  it('goes quiet after cancel', () => {
    const tween = createCameraTween({ durationMs: 100 });
    tween.start(from, to, 0);
    tween.cancel();
    expect(tween.active).toBe(false);
    expect(tween.sample(50)).toBeNull();
  });
});
