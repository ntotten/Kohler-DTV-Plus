import * as THREE from 'three';
import { describe, expect, it } from 'vitest';
import { nearestRegionDirection, stepDirection, type GizmoStep } from './viewGizmo';

// The gizmo's maths, tested without a WebGL context. `createViewGizmo` needs a
// canvas; these two exports deliberately do not, because they are the part that
// can be silently wrong — a mis-signed rotation still looks like a rotation.

/** A camera looking at the origin from `from`, matrices up to date. */
function cameraLookingFrom(
  from: [number, number, number],
  up = [0, 1, 0],
): THREE.PerspectiveCamera {
  const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
  camera.position.set(...from);
  camera.up.set(up[0], up[1], up[2]);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  return camera;
}

const ORIGIN = new THREE.Vector3();
const near = (v: THREE.Vector3, x: number, y: number, z: number) => {
  expect(v.x).toBeCloseTo(x, 6);
  expect(v.y).toBeCloseTo(y, 6);
  expect(v.z).toBeCloseTo(z, 6);
};

describe('nearestRegionDirection', () => {
  it('snaps a near-axis direction to that face', () => {
    near(nearestRegionDirection(new THREE.Vector3(0.1, 0.05, 0.9)), 0, 0, 1);
  });

  it('snaps a diagonal to the edge chamfer between two faces', () => {
    const r = Math.SQRT1_2;
    near(nearestRegionDirection(new THREE.Vector3(0.7, 0.02, 0.71)), r, 0, r);
  });

  it('snaps a three-way diagonal to the corner chamfer', () => {
    const r = 1 / Math.sqrt(3);
    near(nearestRegionDirection(new THREE.Vector3(0.6, 0.55, 0.58)), r, r, r);
  });

  it('covers all 26 regions and nothing else', () => {
    // Every region must be reachable, or part of the cube is decorative.
    const reached = new Set<string>();
    for (let x = -1; x <= 1; x++) {
      for (let y = -1; y <= 1; y++) {
        for (let z = -1; z <= 1; z++) {
          if (!x && !y && !z) continue;
          const probe = new THREE.Vector3(x, y, z).normalize().multiplyScalar(2);
          const snapped = nearestRegionDirection(probe);
          reached.add(
            snapped
              .toArray()
              .map((n) => n.toFixed(4))
              .join(','),
          );
        }
      }
    }
    expect(reached.size).toBe(26);
  });
});

describe('stepDirection', () => {
  it('turns right by 90 degrees onto the adjacent face', () => {
    // Looking from display +Z with +Y up; a step right lands on +X.
    const camera = cameraLookingFrom([0, 0, 10]);
    near(stepDirection('right', camera, ORIGIN), 1, 0, 0);
  });

  it('turns left onto the opposite adjacent face', () => {
    const camera = cameraLookingFrom([0, 0, 10]);
    near(stepDirection('left', camera, ORIGIN), -1, 0, 0);
  });

  it('turns up onto the top face', () => {
    const camera = cameraLookingFrom([0, 0, 10]);
    near(stepDirection('up', camera, ORIGIN), 0, 1, 0);
  });

  it('turns down onto the bottom face', () => {
    const camera = cameraLookingFrom([0, 0, 10]);
    near(stepDirection('down', camera, ORIGIN), 0, -1, 0);
  });

  it('returns to where it started after four steps in one direction', () => {
    let camera = cameraLookingFrom([0, 0, 10]);
    let direction = new THREE.Vector3(0, 0, 1);
    for (let i = 0; i < 4; i++) {
      direction = stepDirection('right', camera, ORIGIN);
      camera = cameraLookingFrom([direction.x * 10, direction.y * 10, direction.z * 10]);
    }
    near(direction, 0, 0, 1);
  });

  it('lands on a clean region even when the camera has been freely orbited', () => {
    // The step rotates the current view then snaps, so an arrow never leaves
    // the camera at some arbitrary angle 90 degrees from another one.
    const camera = cameraLookingFrom([7.3, 2.1, 6.4]);
    const stepped = stepDirection('right', camera, ORIGIN);
    const components = stepped.toArray().map((n) => Math.abs(n));
    const distinct = new Set(components.map((n) => n.toFixed(4)));
    // Every region direction has components drawn from {0, 1}, {0, 0.7071} or
    // all-equal — so at most two distinct magnitudes.
    expect(distinct.size).toBeLessThanOrEqual(2);
    expect(stepped.length()).toBeCloseTo(1, 6);
  });

  it('steps consistently from a top-down view, where up is not +Y', () => {
    const camera = cameraLookingFrom([0, 10, 0], [0, 0, -1]);
    const stepped = stepDirection('down', camera, ORIGIN);
    expect(stepped.length()).toBeCloseTo(1, 6);
    // From directly above, a downward step must leave the pole.
    expect(Math.abs(stepped.y)).toBeLessThan(0.999);
  });
});

const STEPS: GizmoStep[] = ['left', 'right', 'up', 'down'];

describe('every step is a real move', () => {
  it('never returns the direction it was given', () => {
    const camera = cameraLookingFrom([0, 0, 10]);
    for (const step of STEPS) {
      const stepped = stepDirection(step, camera, ORIGIN);
      expect(stepped.dot(new THREE.Vector3(0, 0, 1))).toBeLessThan(0.5);
    }
  });
});
