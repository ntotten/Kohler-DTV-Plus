# B01 — One outlet tap sends one valve command

> Scout-authored batch plan, 2026-08-04. Format per `E:\git\llm-fab\.fab\process\BATCH-DEV.md`.
> This repo has no `fab.json`; drop this into the work directory of whichever run adopts it.
> Corresponds to `KDTV-W2` in [`kohler-dtv-plus-work-scout.md`](kohler-dtv-plus-work-scout.md).

Problem: Tapping an outlet while the shower is running sends `quick_shower.cgi` twice in `npm run dev`, which is the documented way to run the app and the operator's only remote way to run the shower.
Cause: `toggleOutlet` issues the command from inside the `setSelection` updater, and `<StrictMode>` double-invokes updater functions in development — an impure updater is exactly what that double-invocation exists to expose.
Fix: Extract the toggle decision into a pure function, apply it outside the updater, and issue the command once from the caller.
Impact: The controller receives one command per tap in every build, and the impurity that produced the duplicate becomes a test failure rather than an invisible extra request.
Risk: Doing nothing leaves duplicate valve commands ~120 ms apart on a controller whose documented failure mode is rapid successive valve commands going unreachable for up to three hours; doing it touches the app's hottest interaction path, so the pure function must preserve today's behaviour exactly.
Goal: A single outlet tap produces exactly one `quick_shower.cgi` request, pinned by a test that fails if a dispatch is ever moved back inside a state updater.

## Inputs

- `app/src/state/useShower.ts:173-186` — `toggleOutlet`, the defect site.
- `app/src/main.tsx:6-10` — `<StrictMode>`, which must stay enabled.
- `app/src/api/model.ts` — where existing pure derivation helpers live and are already tested.
- `research/FIELD-NOTES.md` §1 — why duplicate valve commands are the consequence that matters.

## Scope

### In scope

- Extracting the toggle decision (next selection, and whether a command follows) into a pure, exported, testable function.
- Rewriting `toggleOutlet` to call it, set state, and dispatch once.
- Tests pinning single-dispatch and updater purity.
- The same audit applied to the other `setSelection` / `setTargetTemp` / `setMassage` call sites in `useShower.ts`, if any share the defect.

### Do not expand

- **Do not remove or weaken `<StrictMode>`.** It is the detector, not the bug. Disabling it hides this class of defect permanently.
- No new test framework, renderer, or DOM library. Vitest is present and sufficient; a React renderer is a new dependency and this batch does not authorize one.
- No change to the polling cadence, the grace window, the debounce, or the safety gate.
- No change to what `quick_shower.cgi` is sent — only how many times.
- No refactor of `useShower.ts` beyond the call sites this defect touches.

## Tasks

### Pin the defect before fixing it

- **B01.T01 — Write the failing purity check first.**
  - Done when: a test asserts that the function passed to `setSelection` in `toggleOutlet` performs no dispatch, and it fails against current `main`.
  - Evidence: `npm test` in `app/` shows the new test red before any source edit; paste the failure.

- **B01.T02 — Confirm the duplicate empirically before changing behaviour.**
  - Done when: the doubled dispatch is observed rather than inferred — a counter or a stubbed command function shows two invocations per simulated toggle under double-invocation, or the attempt is recorded as infeasible without a renderer and the reason stated.
  - Evidence: test output, or a written note in the handoff naming what blocked it. Do not silently skip this; the scout flagged the doubling as inferred from React's documented contract and never measured.

### Fix

- **B01.T03 — Extract the toggle decision into a pure function.**
  - Done when: an exported function takes the current selection, the tapped position and the running flag, and returns the next selection plus whether a command should follow — with no imports of the API client and no side effects.
  - Evidence: unit tests covering add, remove, running and idle cases; calling it twice with identical input produces identical output and no dispatch.

- **B01.T04 — Rewrite `toggleOutlet` to use it.**
  - Done when: the next selection is computed outside the updater, `setSelection` receives a value or a pure updater, and the command is issued exactly once from the caller.
  - Evidence: `npm run check` passes; B01.T01's purity test is now green.

- **B01.T05 — Audit the sibling state-setting call sites.**
  - Done when: `adjustTemp`, `changeMassage` and `start` are checked for the same pattern, and each is either confirmed clean or fixed the same way.
  - Evidence: a one-line verdict per call site in the handoff, with file:line.

### Prove it stays fixed

- **B01.T06 — Pin single-dispatch as a regression test.**
  - Done when: a test fails if a dispatch is reintroduced inside any state updater in `useShower.ts`.
  - Evidence: the test passes on the fixed source, and fails when the fix is temporarily reverted. Show both.

## Validation

- `npm test` in `app/` — all tests pass, including the new ones (56 tests before this batch; the count rises).
- `npm run check` in `app/` — typecheck, tests, build.
- `npm run selftest` in `app/` — strictly read-only, confirms the gate and the live reads still hold. **Never open a valve.**
- Manual, operator-gated only: with water running, one outlet tap produces one entry in the proxy's request path. **Do not perform this check yourself — it moves water and requires explicit in-the-moment operator consent per `AGENT.md`.**

## Hard exit criterion

`npm run check` exits 0 in `app/`.
