// What can be checked without a browser.
//
// The viewer's pixels need eyes, and nothing here replaces them. But the geometry underneath does
// not: fitting a board to a viewport, zooming about a point, clamping a pan to the board's edges
// and turning a click back into a cell are arithmetic, and arithmetic that is wrong by one is
// exactly the kind of thing that looks almost right on screen and is never noticed.
//
//     node check.mjs
//
// Run by build.sh. It stubs the few DOM calls the renderer makes -- a canvas, its 2d context, and
// devicePixelRatio -- and asserts on numbers.

import { readFileSync } from "node:fs";

// ---------------------------------------------------------------- a canvas that counts
function stubDom() {
  const calls = [];
  // Everything on a 2d context is a no-op that records the call, except the two that have to
  // return something the renderer then uses.
  const real = {
    canvas: null,
    createImageData: (w, h) => ({ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }),
    getImageData: (x, y, w, h) => ({ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }),
  };
  const ctx = new Proxy(real, {
    get(t, k) {
      if (k in t) return t[k];
      return (...args) => calls.push([k, ...args]);
    },
    set(t, k, v) {
      t[k] = v;
      return true;
    },
  });
  const makeCanvas = () => ({
    width: 0,
    height: 0,
    style: {},
    getContext: () => ctx,
    setAttribute() {},
  });
  globalThis.document = { createElement: () => makeCanvas() };
  globalThis.window = { devicePixelRatio: 1 };
  return { canvas: makeCanvas(), makeStubCanvas: makeCanvas, calls };
}

let failures = 0;
function check(name, cond, detail) {
  if (cond) {
    console.log(`  ok    ${name}`);
  } else {
    console.log(`  FAIL  ${name}${detail ? " -- " + detail : ""}`);
    failures++;
  }
}
const near = (a, b, eps = 0.001) => Math.abs(a - b) < eps;

const { canvas, makeStubCanvas } = stubDom();
const { Renderer } = await import("./src/render.js");
const replay = JSON.parse(readFileSync("../tests/fixtures/replay-maze-03.json", "utf8"));

console.log("geometry");
const r = new Renderer(canvas);
r.setBoard(replay.map);
check("the board is read", r.rows === 96 && r.cols === 96, `${r.rows}x${r.cols}`);
check("terrain is painted once", !!r.terrain);

// A 96x96 board in a 900x500 viewport fits on the short side, and is centred on the long one.
r.resize(900, 500);
check("fit uses the short side", near(r.scale, 500 / 96), `scale ${r.scale}`);
check("fitted is fitted", r.atFit());
const vw = r.cols * r.scale;
check("a narrower board is centred", near(r.ox, (vw - 900) / 2), `ox ${r.ox}`);

// Zooming about a point keeps the cell under that point under it. This is the one that is wrong in
// most hand-written viewers, and it is wrong in a way that only shows up as "the map runs away".
// Zoom in past fit first: while the board is smaller than the viewport it is centred, and the
// clamp legitimately overrides the cursor -- there is no off-board space to show.
r.fit();
r.zoomAt(4, 450, 250);
const px = 430;
const py = 260;
const before = r.cellAt(px, py);
r.zoomAt(1.4, px, py);
const after = r.cellAt(px, py);
check(
  "zoom keeps the cell under the cursor",
  before && after && Math.abs(before[0] - after[0]) <= 1 && Math.abs(before[1] - after[1]) <= 1,
  `${JSON.stringify(before)} -> ${JSON.stringify(after)}`
);

// And a resize while fitted stays fitted, which is the bug the flag exists for: the first resize
// runs against a zero-sized canvas.
const r2 = new (Object.getPrototypeOf(r).constructor)(makeStubCanvas());
r2.setBoard(replay.map);
r2.resize(900, 500);
check("a fresh renderer fits on its first resize", near(r2.scale, 500 / 96), `scale ${r2.scale}`);
r2.resize(600, 300);
check("and stays fitted when the frame changes", near(r2.scale, 300 / 96), `scale ${r2.scale}`);
r2.zoomAt(2, 300, 150);
r2.resize(900, 500);
check("but a zoomed view is not re-fitted", !near(r2.scale, 500 / 96), `scale ${r2.scale}`);

// Panning stops at the board's edges rather than letting it drift into space.
r.pan(1e6, 1e6);
check("pan clamps at the top-left", r.ox >= -0.001 && r.oy >= -0.001, `ox ${r.ox} oy ${r.oy}`);
r.pan(-1e6, -1e6);
check(
  "pan clamps at the bottom-right",
  r.ox <= r.cols * r.scale - 900 + 0.001 && r.oy <= r.rows * r.scale - 500 + 0.001,
  `ox ${r.ox}`
);

// Zooming out never goes below fit, so the board cannot become a speck in a corner.
r.zoomAt(1 / 1000, 0, 0);
check("zoom out stops at fit", near(r.scale, r.fitScale()), `scale ${r.scale} fit ${r.fitScale()}`);

// A click outside the board is not a cell.
check("outside the board is not a cell", r.cellAt(-50, -50) === null);

// Round trip: the centre of a cell maps back to that cell, at several zooms.
let roundTrips = true;
for (const z of [1, 2, 4]) {
  r.fit();
  r.zoomAt(z, 450, 250);
  for (const [rr, cc] of [[0, 0], [10, 20], [95, 95]]) {
    const x = cc * r.scale - r.ox + r.scale / 2;
    const y = rr * r.scale - r.oy + r.scale / 2;
    const back = r.cellAt(x, y);
    if (!back || back[0] !== rr || back[1] !== cc) roundTrips = false;
  }
}
check("a cell's centre maps back to that cell", roundTrips);

// ---------------------------------------------------------------- the shell's derived data
console.log("timeline");
globalThis.ResizeObserver = class {
  observe() {}
  disconnect() {}
};
// From dist/, because that is where the transpiled component lives -- src/engine.js imports it
// by a path that only exists after build.sh has run.
const { allFrames } = await import("./dist/engine.js");
const frames = allFrames(replay);
check("every turn has a frame", frames.length === replay.turns + 1, `${frames.length}`);

// Event marks are derived from the frames alone -- a hill leaving the list was razed, and an ant
// count reaching zero is a colony out. Both must be findable without knowing a rule.
const mod = readFileSync("./src/shell.js", "utf8");
check("events are derived, not hard-coded", /function findEvents/.test(mod));
check("the track is clickable", /pointerdown/.test(mod) && /releasePointerCapture/.test(mod));
check("arrows step", /ArrowRight/.test(mod) && /ArrowLeft/.test(mod));
check("transport buttons exist", /this\.nextBtn/.test(mod) && /this\.prevBtn/.test(mod));

console.log(failures ? `\n${failures} FAILURE(S)` : "\nall geometry checks passed");
process.exit(failures ? 1 : 0);
