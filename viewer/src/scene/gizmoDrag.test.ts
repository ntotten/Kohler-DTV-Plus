import { describe, expect, it } from 'vitest';
import { CLICK_SLOP_PX, COARSE_CLICK_SLOP_PX, createDragProbe, dragThresholdPx } from './gizmoDrag';

// The click-versus-drag decision, tested without a pointer or a camera. It is
// the part of drag-to-orbit that can be silently wrong: a threshold that is too
// eager turns taps into one-degree lurches, and a probe that forgets how far it
// has been snaps the camera away at the end of a long orbit. Both look like the
// gizmo misbehaving rather than like an off-by-one in a comparison.

describe('dragThresholdPx', () => {
  it('allows a mouse the fine slop', () => {
    expect(dragThresholdPx('mouse')).toBe(CLICK_SLOP_PX);
  });

  it('allows touch and pen more, because they wander', () => {
    expect(dragThresholdPx('touch')).toBe(COARSE_CLICK_SLOP_PX);
    expect(dragThresholdPx('pen')).toBe(COARSE_CLICK_SLOP_PX);
  });

  it('treats an absent pointerType as a mouse', () => {
    // Synthetic events and older browsers leave it blank; a desktop user is the
    // likelier case, and guessing coarse would make every real click sloppy.
    expect(dragThresholdPx(undefined)).toBe(CLICK_SLOP_PX);
  });

  it('keeps the coarse slop comfortably above the fine one', () => {
    expect(COARSE_CLICK_SLOP_PX).toBeGreaterThan(CLICK_SLOP_PX);
  });
});

describe('createDragProbe', () => {
  it('starts as a click', () => {
    const probe = createDragProbe(100, 100, 4);
    expect(probe.isClick).toBe(true);
    expect(probe.travelPx).toBe(0);
  });

  it('stays a click through a wobble under the threshold', () => {
    const probe = createDragProbe(100, 100, 4);
    expect(probe.moved(102, 100)).toBe(false);
    expect(probe.moved(100, 101)).toBe(false);
    expect(probe.moved(101, 102)).toBe(false);
    expect(probe.isClick).toBe(true);
  });

  it('becomes a drag once the threshold is passed', () => {
    const probe = createDragProbe(100, 100, 4);
    expect(probe.moved(110, 100)).toBe(true);
    expect(probe.isClick).toBe(false);
  });

  it('gives the boundary to the click', () => {
    // Exactly `threshold` away is not past it. A 3-4-5 triangle so the distance
    // is exact in floating point and the test is about the comparison, not
    // about rounding.
    const probe = createDragProbe(0, 0, 5);
    expect(probe.moved(3, 4)).toBe(false);
    expect(probe.isClick).toBe(true);
    expect(probe.moved(3.01, 4)).toBe(true);
  });

  it('measures diagonally, not per axis', () => {
    // 3 px right and 3 px down is 4.24 px of travel, which is over a 4 px slop
    // even though neither component is.
    const probe = createDragProbe(0, 0, 4);
    expect(probe.moved(3, 3)).toBe(true);
  });

  it('latches: coming back to the start does not make it a click again', () => {
    // The failure this guards: orbit right, orbit back, release near where you
    // pressed — and the camera abandons the view you just arrived at to snap to
    // whichever face you happened to grab.
    const probe = createDragProbe(100, 100, 4);
    expect(probe.moved(200, 100)).toBe(true);
    expect(probe.moved(100, 100)).toBe(true);
    expect(probe.isClick).toBe(false);
  });

  it('reports the furthest travel, not the latest', () => {
    const probe = createDragProbe(0, 0, 4);
    probe.moved(0, 60);
    probe.moved(0, 10);
    expect(probe.travelPx).toBeCloseTo(60, 6);
  });

  it('handles a coarse pointer at its own threshold', () => {
    const probe = createDragProbe(0, 0, COARSE_CLICK_SLOP_PX);
    // A 6 px finger drift is a drag for a mouse and a tap for a finger.
    expect(probe.moved(6, 0)).toBe(false);
    expect(probe.moved(12, 0)).toBe(true);
  });
});
