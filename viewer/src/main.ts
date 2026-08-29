import './style.css';
import catalogJson from './catalog/catalog.json';
import decalsJson from './catalog/decals.json';
import {
  entryKey,
  findEntry,
  flattenCatalog,
  searchEntries,
  validateCatalog,
} from './catalog/catalog';
import type { CatalogEntry } from './catalog/types';
import { computeMeshStats, estimatedMassGrams, volumeCm3, type MeshStats } from './core/meshStats';
import { exportFilename, exportHeader, readBinaryStlCount, writeBinaryStl } from './core/stl';
import { repairMesh, summarizeRepair, type RepairResult } from './core/repair';
import {
  decalQuad,
  decalsFor,
  summarizeDecal,
  validateDecalSet,
  type DecalRecord,
} from './core/decals';
import { formatDimensions, scaleFactor, type LengthUnit, type UpAxis } from './core/units';
import { detectFormat, isTextFormat, loadModel, parseModel } from './scene/loaders';
import { createViewer, isWebglSupported, type LoadedModel } from './scene/viewer';
import type { ViewName } from './scene/cameraFit';

// Entry point. Owns the DOM, the current selection, and the export button.
// `core/` is pure and tested; `scene/` owns GPU resources; this file is the
// wiring between them and should stay free of geometry logic.

const $ = <T extends HTMLElement>(id: string): T => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing element #${id}`);
  return el as T;
};

const statusEl = $('status');
const readoutEl = $('readout');
const catalogEl = $('catalog');
const searchEl = $<HTMLInputElement>('search');
const downloadEl = $<HTMLButtonElement>('download');
const exportUnitEl = $<HTMLSelectElement>('export-unit');
const exportMeshEl = $<HTMLSelectElement>('export-mesh');
const exportNoteEl = $('export-note');
const statsEl = $('stats');
const decalsEl = $('decals');
const decalSelectEl = $<HTMLSelectElement>('decal');
const decalViewEl = $<HTMLButtonElement>('decal-view');
const decalNoteEl = $('decal-note');

/** Everything about what is currently on screen. */
interface Current {
  label: string;
  model: LoadedModel;
  stats: MeshStats;
  /** Repaired geometry and its report. Computed once at load. */
  repair: RepairResult;
  repairedStats: MeshStats;
}

let current: Current | null = null;

function setStatus(message: string, isError = false): void {
  statusEl.textContent = message;
  statusEl.classList.toggle('error', isError);
  statusEl.hidden = message === '';
}

// ---------------------------------------------------------------- catalog

const catalog = validateCatalog(catalogJson);
const entries = flattenCatalog(catalog);

// Decals are validated at boot, next to the catalog, for the same reason: a
// sheared anchor or a squashed aspect ratio renders perfectly and is wrong.
const decalSet = validateDecalSet(decalsJson);

function renderCatalog(query = ''): void {
  const visible = searchEntries(entries, query);
  catalogEl.replaceChildren();

  if (visible.length === 0) {
    const empty = document.createElement('p');
    empty.className = 'empty';
    empty.textContent = 'No parts match that search.';
    catalogEl.append(empty);
    return;
  }

  // Group by family, preserving catalog order rather than sorting — the
  // catalog's own order is editorial.
  const byFamily = new Map<string, CatalogEntry[]>();
  for (const entry of visible) {
    const list = byFamily.get(entry.family.familyId) ?? [];
    list.push(entry);
    byFamily.set(entry.family.familyId, list);
  }

  for (const [, familyEntries] of byFamily) {
    const section = document.createElement('div');
    section.className = 'family';

    const heading = document.createElement('h4');
    heading.textContent = `${familyEntries[0].family.brand} — ${familyEntries[0].family.title}`;
    section.append(heading);

    for (const entry of familyEntries) {
      const button = document.createElement('button');
      button.className = 'part';
      button.dataset.key = entryKey(entry);

      const title = document.createElement('span');
      title.textContent = entry.part.title;
      button.append(title);

      if (entry.part.sku) {
        const sku = document.createElement('span');
        sku.className = 'sku';
        sku.textContent = entry.part.sku;
        button.append(sku);
      }

      button.addEventListener('click', () => {
        void selectEntry(entry);
      });
      section.append(button);
    }
    catalogEl.append(section);
  }

  markSelected();
}

function markSelected(): void {
  const key = location.hash.slice(1);
  for (const button of catalogEl.querySelectorAll<HTMLButtonElement>('.part')) {
    button.setAttribute('aria-current', String(button.dataset.key === key));
  }
}

// ---------------------------------------------------------------- viewer

if (!isWebglSupported()) {
  setStatus('This browser has no WebGL support, so the 3D view is unavailable.', true);
}

const viewer = createViewer($<HTMLCanvasElement>('canvas'), $('canvas-wrap'));

for (const button of document.querySelectorAll<HTMLButtonElement>('[data-view]')) {
  button.addEventListener('click', () => viewer.snap(button.dataset.view as ViewName));
}
$('fit').addEventListener('click', () => viewer.fit());

const toggle = (id: string, apply: (on: boolean) => void): void => {
  const input = $<HTMLInputElement>(id);
  input.addEventListener('change', () => apply(input.checked));
};
toggle('wireframe', (on) => viewer.setWireframe(on));
toggle('flat', (on) => viewer.setFlatShading(on));
toggle('grid', (on) => viewer.setGridVisible(on));
toggle('bounds', (on) => viewer.setBoundsVisible(on));

// Pointer readout. Reports the picked point in EXPORT space (mm, Z-up) — the
// same frame the downloaded STL uses, so a coordinate read here can be typed
// straight into CAM without a mental conversion.
$('canvas').addEventListener('pointermove', (event) => {
  const hit = viewer.pick(event.clientX, event.clientY);
  if (!hit) {
    readoutEl.hidden = true;
    return;
  }
  const [x, y, z] = hit.point;
  readoutEl.hidden = false;
  readoutEl.innerHTML =
    `<span class="axis">X</span> ${x.toFixed(2)} ` +
    `<span class="axis">Y</span> ${y.toFixed(2)} ` +
    `<span class="axis">Z</span> ${z.toFixed(2)} ` +
    `<span class="axis">mm</span>`;
});

// ---------------------------------------------------------------- loading

async function selectEntry(entry: CatalogEntry): Promise<void> {
  const label = entry.part.sku ?? entry.part.title;
  setStatus(`Loading ${entry.file.name}…`);
  history.replaceState(null, '', `#${entryKey(entry)}`);
  markSelected();

  $('part-title').textContent = entry.part.title;
  $('part-sku').textContent = entry.part.sku ?? '';
  $('part-desc').textContent = entry.part.description ?? '';

  const modNotes = $('mod-notes');
  modNotes.hidden = !entry.part.modNotes;
  $('mod-notes-body').textContent = entry.part.modNotes ?? '';
  $('provenance-body').textContent =
    entry.file.provenanceNote ??
    'No provenance note recorded for this file. Units and orientation are taken from the catalog declaration.';

  try {
    const object = await loadModel(entry.file.url, entry.file.format);
    applyModel(label, object, entry.file.sourceUnit, entry.file.sourceUpAxis);
    // After applyModel: setting a model clears the decal layer, because decals
    // are pinned to one specific file's geometry.
    await applyDecals(`${entry.family.familyId}/${entry.part.partId}/${entry.file.id}`);
  } catch (error) {
    onLoadError(entry.file.url, error);
  }
}

function applyModel(
  label: string,
  object: import('three').Object3D,
  unit: LengthUnit,
  upAxis: UpAxis,
): void {
  const model = viewer.setModel(object, unit, upAxis);
  const stats = computeMeshStats(model.sourceGeometry, {
    sourceUnit: unit,
    sourceUpAxis: upAxis,
  });

  // Repair eagerly rather than on click. It is fast at these triangle counts,
  // and the report is a measurement of the part someone should see BEFORE
  // deciding which file to download — not a surprise after the fact.
  const repair = repairMesh(model.sourceGeometry, unit);
  const repairedStats = computeMeshStats(repair.geometry, {
    sourceUnit: unit,
    sourceUpAxis: upAxis,
  });

  current = { label, model, stats, repair, repairedStats };
  renderStats(stats);
  renderRepairReport(repair, stats, repairedStats);
  downloadEl.disabled = false;
  updateExportNote();
  setStatus('');
}

// ---------------------------------------------------------------- decals

/**
 * Decals are artwork pinned to a face of the part — a screen, a legend, a
 * label. They are added content sitting on top of manufacturer geometry, so
 * the picker states what is on screen and the note underneath says where it
 * came from. Nothing here can reach the export: the decal layer is a sibling of
 * the model in the scene graph, and the exporter reads the source geometry
 * snapshot taken before either existed.
 */
async function applyDecals(sourceModelId: string): Promise<void> {
  const records = decalsFor(decalSet, sourceModelId);
  decalsEl.hidden = records.length === 0;
  decalSelectEl.replaceChildren();
  if (records.length === 0) {
    viewer.selectDecal(null);
    return;
  }

  const none = document.createElement('option');
  none.value = '';
  none.textContent = 'None — bare geometry';
  decalSelectEl.append(none);

  for (const record of records) {
    const option = document.createElement('option');
    option.value = record.decalId;
    option.textContent = record.title;
    decalSelectEl.append(option);
  }

  const loaded = await viewer.setDecals(records);
  // Default to the first decal that actually loaded. A decal whose image is
  // missing is dropped by the layer rather than selected as an invisible quad.
  const first = loaded[0]?.record.decalId ?? '';
  decalSelectEl.value = first;
  viewer.selectDecal(first || null);
  renderDecalNote(records.find((r) => r.decalId === first));
}

function renderDecalNote(record: DecalRecord | undefined): void {
  decalViewEl.disabled = !record;
  if (!record) {
    decalNoteEl.textContent =
      'Decals are added artwork floated off a measured face. The downloaded STL never ' +
      'contains them.';
    return;
  }
  decalNoteEl.textContent = `${summarizeDecal(record, decalQuad(record))}. ${record.provenanceNote}`;
}

decalSelectEl.addEventListener('change', () => {
  const id = decalSelectEl.value;
  viewer.selectDecal(id || null);
  renderDecalNote(decalSet.decals.find((r) => r.decalId === id));
});

/**
 * Look at the selected decal square-on and the right way up.
 *
 * The standard views are CAD views, and a CAD view has no idea which way the
 * product stands: the K-99693 is modelled on its side, so "Front" shows the
 * wall bracket and every other view has the interface lying down. The decal's
 * own anchor knows better — `v` is the artwork's up vector — so this needs no
 * per-part configuration and works for any decal on any part.
 */
decalViewEl.addEventListener('click', () => {
  const record = decalSet.decals.find((r) => r.decalId === decalSelectEl.value);
  if (!record) return;
  viewer.lookAtDecal(decalQuad(record).normal, record.anchor.v);
});

function onLoadError(what: string, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  setStatus(`Could not load ${what}: ${message}`, true);
  current = null;
  downloadEl.disabled = true;
  statsEl.replaceChildren();
  decalsEl.hidden = true;
  viewer.selectDecal(null);
}

// ---------------------------------------------------------------- stats

function renderStats(stats: MeshStats): void {
  const rows: Array<[string, string, boolean?]> = [
    ['Envelope', formatDimensions(stats.size, stats.unit, 2)],
    ['Triangles', stats.triangleCount.toLocaleString()],
    ['Vertices', stats.vertexCount.toLocaleString()],
    ['Surface area', `${(stats.surfaceArea / 100).toFixed(2)} cm²`],
  ];

  if (stats.closed) {
    rows.push(['Volume', `${volumeCm3(stats).toFixed(2)} cm³`]);
    rows.push(['Solid PLA', `${estimatedMassGrams(stats).toFixed(1)} g`]);
    rows.push(['Mesh', 'closed']);
  } else {
    // An open mesh's volume is not a number anyone should act on, so it is
    // withheld rather than shown with a caveat nobody reads.
    rows.push(['Mesh', `open — ${stats.boundaryEdges.toLocaleString()} unshared edges`, true]);
  }

  statsEl.replaceChildren();
  for (const [term, value, warn] of rows) {
    const dt = document.createElement('dt');
    dt.textContent = term;
    const dd = document.createElement('dd');
    dd.textContent = value;
    if (warn) dd.className = 'warn';
    statsEl.append(dt, dd);
  }
}

/**
 * Repair report, shown alongside the measurements so the difference between the
 * two downloads is visible before either is chosen.
 */
function renderRepairReport(repair: RepairResult, before: MeshStats, after: MeshStats): void {
  const { report } = repair;
  const panel = $('repair-report');
  const stats = $('repair-stats');
  const verdict = $('repair-verdict');

  // Nothing was wrong and nothing was done — the panel would be noise.
  if (before.closed && report.trianglesAdded === 0 && report.collapsedEdges === 0) {
    panel.hidden = true;
    return;
  }
  panel.hidden = false;

  const drift = Math.max(...after.size.map((s, i) => Math.abs(s - before.size[i])));
  const rows: Array<[string, string, boolean?]> = [
    ['Holes capped', `${report.loopsCapped} of ${report.loopsFound}`],
    ['Vertices welded', report.weldedVertices.toLocaleString()],
    ['Triangles added', `+${report.trianglesAdded}`],
    ['Boundary edges', `${report.boundaryEdgesBefore} → ${report.boundaryEdgesAfter}`],
  ];
  if (report.collapsedEdges) {
    rows.push(['Seams collapsed', String(report.collapsedEdges)]);
  }
  rows.push(['Non-manifold', String(report.nonManifoldEdges), report.nonManifoldEdges > 0]);
  // The number that decides whether the repair is safe to machine against.
  rows.push(['Envelope drift', `${drift.toFixed(4)} mm`, drift > 0.001]);
  if (after.closed) {
    rows.push(['Enclosed volume', `${volumeCm3(after).toFixed(2)} cm³`]);
  }

  stats.replaceChildren();
  for (const [term, value, warn] of rows) {
    const dt = document.createElement('dt');
    dt.textContent = term;
    const dd = document.createElement('dd');
    dd.textContent = value;
    if (warn) dd.className = 'warn';
    stats.append(dt, dd);
  }

  verdict.textContent = summarizeRepair(report);
  verdict.classList.toggle('warn', !report.watertight);

  if (report.skipped.length) {
    const detail = report.skipped
      .map((s) => `${s.points} pts, ${s.planarityMm.toFixed(2)} mm out of plane`)
      .join('; ');
    verdict.textContent += ` Skipped: ${detail}.`;
  }
}

function updateExportNote(): void {
  if (!current) {
    exportNoteEl.textContent = '';
    return;
  }
  const unit = exportUnitEl.value as LengthUnit;
  const from = current.model.sourceUnit;
  const factor = scaleFactor(from, unit);
  const scaling = factor === 1 ? 'no scaling' : `scaled x${Number(factor.toFixed(6))}`;
  const repaired = exportMeshEl.value === 'repaired';
  const mesh = repaired
    ? current.repair.report.watertight
      ? 'Repaired mesh: watertight and manifold.'
      : 'Repaired mesh: still NOT watertight — see the report below.'
    : 'As published: unmodified manufacturer geometry.';

  exportNoteEl.textContent =
    `Source is ${from}, ${current.model.sourceUpAxis.toUpperCase()}-up. ` +
    `Export is ${unit}, Z-up, ${scaling}. ${mesh} ` +
    `STL records no units, so the filename states them.`;
}

exportUnitEl.addEventListener('change', updateExportNote);
exportMeshEl.addEventListener('change', updateExportNote);

// ---------------------------------------------------------------- export

downloadEl.addEventListener('click', () => {
  if (!current) return;
  const unit = exportUnitEl.value as LengthUnit;
  const repaired = exportMeshEl.value === 'repaired';

  // The filename records which mesh this is. Two files that differ in
  // watertightness but share a name is exactly how the wrong one ends up on the
  // machine.
  const label = repaired ? `${current.label} repaired` : current.label;
  const geometry = repaired ? current.repair.geometry : current.model.sourceGeometry;

  try {
    const result = writeBinaryStl(geometry, {
      sourceUnit: current.model.sourceUnit,
      sourceUpAxis: current.model.sourceUpAxis,
      targetUnit: unit,
      header: exportHeader(label, unit),
    });

    // Verify the bytes we are about to hand over actually parse as a binary
    // STL. Cheap, and it turns a corrupt download into an error message here
    // rather than a failed print an hour later.
    readBinaryStlCount(result.buffer);

    downloadBuffer(result.buffer, exportFilename(label, unit));
    setStatus(
      `Exported ${result.triangleCount.toLocaleString()} triangles at ` +
        `${result.scale === 1 ? '1:1' : `x${result.scale}`} into ${unit}, Z-up` +
        `${repaired ? ', repaired' : ', as published'}.`,
    );
  } catch (error) {
    setStatus(`Export failed: ${error instanceof Error ? error.message : String(error)}`, true);
  }
});

function downloadBuffer(buffer: ArrayBuffer, filename: string): void {
  const url = URL.createObjectURL(new Blob([buffer], { type: 'model/stl' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  // Revoke on the next task: revoking synchronously can race the download in
  // some browsers and yield a zero-byte file.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

// ---------------------------------------------------------------- drop

const body = document.body;

for (const type of ['dragenter', 'dragover'] as const) {
  body.addEventListener(type, (event) => {
    event.preventDefault();
    body.classList.add('dragging');
  });
}
for (const type of ['dragleave', 'drop'] as const) {
  body.addEventListener(type, () => body.classList.remove('dragging'));
}

body.addEventListener('drop', (event) => {
  event.preventDefault();
  const file = event.dataTransfer?.files?.[0];
  if (file) void loadDroppedFile(file);
});

/**
 * Load a user-supplied file.
 *
 * Units and up-axis cannot be known for a dropped file — no mesh format records
 * them — so the current selector values are used and the status line says so
 * plainly. A dropped model is for inspection; anything destined for a machine
 * should go through a catalog entry where the units have been established.
 */
async function loadDroppedFile(file: File): Promise<void> {
  const format = detectFormat(file.name);
  if (!format) {
    setStatus(
      `${file.name}: unrecognised extension. Supported: obj, stl, 3mf, gltf, glb, 3ds, ply.`,
      true,
    );
    return;
  }

  setStatus(`Loading ${file.name}…`);
  try {
    const data = isTextFormat(format) ? await file.text() : await file.arrayBuffer();
    const object = await parseModel(data, format);

    $('part-title').textContent = file.name;
    $('part-sku').textContent = 'Dropped file';
    $('part-desc').textContent = '';
    $('mod-notes').hidden = true;
    $('provenance-body').textContent =
      'Dropped file. No mesh format records its units or up-axis, so millimetres and ' +
      'Z-up are assumed. Confirm against a dimensioned drawing before cutting anything.';

    applyModel(file.name.replace(/\.[^.]+$/, ''), object, 'mm', 'z');
    // A dropped file has no catalog identity, so no decal can be pinned to it.
    await applyDecals('');
    setStatus('Loaded as millimetres, Z-up — assumed, not read from the file.');
  } catch (error) {
    onLoadError(file.name, error);
  }
}

// ---------------------------------------------------------------- boot

searchEl.addEventListener('input', () => renderCatalog(searchEl.value));
renderCatalog();

const initial = findEntry(entries, location.hash.slice(1)) ?? entries[0];
if (initial) void selectEntry(initial);
else setStatus('Catalog is empty. Drop a model file to view it.');
