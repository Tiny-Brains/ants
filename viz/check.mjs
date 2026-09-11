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
    createRadialGradient: (...args) => {
      calls.push(["createRadialGradient", ...args]);
      return { addColorStop() {} };
    },
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

const { canvas, makeStubCanvas, calls } = stubDom();
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

// ---------------------------------------------------------------- the stylesheet
//
// The viewer injects ONE <style> into the host document -- it is not a shadow root -- so a rule
// that does not start at .tb-viz is a rule that can land on the page around it. An unscoped
// `.tb-bar` in here once relaid out the web application's own header the moment a replay mounted,
// and the way that bug reads from the outside is "the site breaks when you open a match".
console.log("stylesheet");
const css = mod.slice(mod.indexOf("const CSS = `") + 13, mod.indexOf("\n`;\n"));
check("the stylesheet was found", css.length > 500, `${css.length} chars`);

// The stylesheet is a template literal, so a backtick inside a CSS comment ends it early and the
// rest of the file is read as JavaScript. `node --check` on a .js file does not catch that -- it
// parses the wreckage as a script and says nothing -- but loading the module does, and the shell
// is the only file here that no other check imports.
let shellLoads = true;
try {
  const shell = await import("./dist/shell.js");
  shellLoads = typeof shell.Viewer === "function";
} catch (e) {
  shellLoads = false;
  console.log(`        ${e.message}`);
}
check("the built shell loads", shellLoads);

/** Split a selector list on the commas that separate selectors, not the ones inside :is(...). */
function splitTop(sel) {
  const out = [];
  let buf = "";
  let depth = 0;
  for (const ch of sel) {
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      out.push(buf);
      buf = "";
    } else buf += ch;
  }
  out.push(buf);
  return out;
}

// Declarations never contain a `{`, so every prelude before one is a selector list or an at-rule.
const preludes = [];
{
  let buf = "";
  for (const ch of css.replace(/\/\*[\s\S]*?\*\//g, "")) {
    if (ch === "{") {
      preludes.push(buf.trim());
      buf = "";
    } else if (ch === "}") buf = "";
    else buf += ch;
  }
}
const escaped = preludes
  .filter((p) => p && !p.startsWith("@"))
  .flatMap(splitTop)
  .map((x) => x.trim())
  .filter((x) => x && !x.startsWith(".tb-viz"));
check("every rule is scoped to .tb-viz", escaped.length === 0, escaped.join(" | "));
check("the chrome reads the page's tokens", /var\(--ink,/.test(css) && /var\(--accent,/.test(css));
check("the board's ground is a literal", /--tb-void:#/.test(css));

// The readouts are a tray over the board: everything but the transport is out of the way until the
// viewer is hovered, focused or touched. A frame is 420 pixels on a match page and the seat strip
// used to spend a fifth of it.
console.log("the tray");
check("the tray exists", /"tb-tray"/.test(mod) && /\.tb-viz \.tb-tray\{/.test(css));
check("it is hidden until wanted", /\.tb-viz \.tb-tray\{[^}]*opacity:0/.test(css));
check(
  "hover, focus and touch raise it",
  /:hover/.test(css) && /:focus-within/.test(css) && /\[data-tb-peek\]/.test(css) && /pointerType === "touch"/.test(mod)
);
check("the transport is not in it", /mk\("div", "tb-bar"\)/.test(mod));
// The seats are the other half of what is always on screen: who is playing and the score are read
// the whole way through, so they sit in a bar of their own above the board rather than in the tray.
check(
  "the seats are a bar of their own, always on screen",
  /this\.seats = mk\("div", "tb-top"\);/.test(mod) &&
    /\.tb-viz \.tb-top\{/.test(css) &&
    !/\.tb-top\{[^}]*opacity/.test(css)
);
check(
  "each seat shows its colour, name and score",
  /"tb-chip"/.test(mod) && /"tb-name"/.test(mod) && /"tb-score"/.test(mod)
);
check("the board's label and the cell readout are gone", !/tb-tip|tb-meta|showTip/.test(mod));
check("seat chips are built once, not per frame", /buildSeats\(\)/.test(mod) && !/this\.seats\.innerHTML = ""/.test(mod));
// Four seats and six do not fit one line of a 505-pixel frame, so the seats are a grid -- and its
// columns come from the seat count and the width alone, so no turn can change how many rows the
// bar takes and move the board under it.
check(
  "the seats' columns are the layout's, not their text's",
  /\.tb-viz \.tb-top\{[^}]*grid-template-columns:repeat\(var\(--tb-cols/.test(css) &&
    /seatColumns\(this\.seatRows\.length, width\)/.test(mod)
);
// "12 ants · 1 hill" took a hundred pixels a seat and was the first thing cut: a 505-pixel frame
// read "1 ant · 1 h…" and never said how many hills anyone had.
check(
  "ants and hills are the board's shapes, not words",
  /"tb-ant"/.test(mod) && /"tb-hill"/.test(mod) && !/row\.nums\.textContent/.test(mod)
);
check(
  "an owner with no room wraps out of sight rather than showing a lone @",
  /\.tb-viz \.tb-id\{[^}]*flex-wrap:wrap[^}]*height:1\.5em[^}]*overflow:hidden/.test(css)
);
check("the stylesheet is per document", /getElementById\(STYLE_ID\)/.test(mod));
// mount() puts `tb-viz` on the host element rather than making a root of its own, and that class
// sets display:flex -- which outranks the [hidden] attribute's UA rule. A host that hides the
// player while it loads a replay depends on this one line.
check("a host can still hide it", /\.tb-viz\[hidden\]\{display:none\}/.test(css));
// The host decides the width. The title bar is a line of text that does not wrap and the canvas is
// sized in pixels, so without containment either became the viewer's minimum width and a grid column
// holding it grew to fit -- the web's home-page replay was 871 pixels wide in a 410-pixel column.
check(
  "the host decides the width, not the viewer's content",
  /\.tb-viz\{[^}]*contain:inline-size/.test(css)
);

// ---------------------------------------------------------------- what is drawn over the board
//
// Both are derived without knowing a rule. A hill is ringed when an enemy is within eight MOVES of
// it -- round water, across the wrap -- and a seat's territory is the fold of what the engine said
// it saw first, turn by turn.
console.log("over the board");
if (shellLoads) {
  const shell = await import("./dist/shell.js");

  // Five rows by seven, wrapping. Column 0 is all water, so the short way round the left is shut;
  // column 3 is water but for the bottom row, so the way across is the long way round.
  const plan = ["#..#...", "#..#...", "#..#...", "#..#...", "#......"];
  const rows = plan.length;
  const cols = plan[0].length;
  const water = Uint8Array.from(plan.join(""), (ch) => (ch === "#" ? 1 : 0));
  const at = (r, c) => r * cols + c;
  const d = shell.stepsFrom(water, rows, cols, at(0, 1), 8);
  check("a step is one move", d[at(0, 2)] === 1);
  check("the board wraps", d[at(4, 1)] === 1, `${d[at(4, 1)]}`);
  check("a wall is walked round, not through", d[at(0, 4)] === 5, `${d[at(0, 4)]}`);
  check("water is never reached", d[at(0, 0)] === -1 && d[at(0, 3)] === -1);
  check("beyond the reach is not counted", shell.stepsFrom(water, rows, cols, at(0, 1), 4)[at(0, 4)] === -1);

  const board = () => ({ rows, cols, water, steps: new Map() });
  const hill = [0, 1, 0];
  const rung = shell.threatsIn({ hills: [hill], ants: [[0, 1, 0], [0, 4, 1]] }, board(), 8);
  check(
    "an enemy within reach rings the hill, in its owner's name",
    rung.length === 1 && rung[0].owner === 0 && rung[0].steps === 5,
    JSON.stringify(rung)
  );
  check("a hill's own ants are no threat to it", shell.threatsIn({ hills: [hill], ants: [[0, 2, 0]] }, board(), 8).length === 0);
  check("an enemy out of reach is no threat yet", shell.threatsIn({ hills: [hill], ants: [[0, 4, 1]] }, board(), 4).length === 0);

  // The fixture, decoded above: every square a seat knows is announced once, on the turn it first
  // saw it, and turn zero carries the opening vision.
  let repeats = 0;
  const known = frames[0].score.map(() => new Set());
  for (const f of frames) {
    (f.discovered ?? []).forEach((list, s) => {
      for (const [r, c] of list) {
        if (known[s].has(r * 96 + c)) repeats++;
        known[s].add(r * 96 + c);
      }
    });
  }
  check(
    "every frame says what each seat saw first",
    frames.every((f) => Array.isArray(f.discovered) && f.discovered.length === f.score.length)
  );
  check("the opening vision is turn zero's", frames[0].discovered.every((l) => l.length > 0));
  check("nothing is announced twice", repeats === 0, `${repeats} repeats`);

  // Territory paints fog where nobody has looked and each seat's colour where it has, mixed where
  // both have -- and keeps each seat's own frontier.
  const rt = new Renderer(makeStubCanvas());
  rt.setBoard(replay.map);
  const mask = new Uint8Array(96 * 96);
  mask[0] = 1; // seat 0 alone at (0,0)
  mask[1] = 3; // both seats at (0,1)
  rt.setTerritory(mask);
  const px = calls.filter((c) => c[0] === "putImageData").at(-1)[1].data;
  check("where nobody has looked is fogged", px[2 * 4 + 3] === 150, `alpha ${px[2 * 4 + 3]}`);
  check("where a seat has is tinted", px[3] === 64 && px[4 + 3] === 64);
  check("where two have is both colours", px[4] > 0x5a && px[4] < 0xff, `red ${px[4]}`);
  check(
    "each seat's frontier is its own",
    rt.frontier[0].length === 6 * 3 && rt.frontier[1].length === 4 * 3,
    `${rt.frontier[0].length / 3} and ${rt.frontier[1].length / 3} edges`
  );

  // A hill in the corner rings all four corners of a board that wraps, and nothing outside it.
  rt.resize(900, 500);
  const before = calls.length;
  let drew = true;
  try {
    rt.render(frames[0], { threats: [{ r: 0, c: 0, owner: 1, steps: 3, reach: 8 }] });
  } catch (e) {
    drew = false;
    console.log(`        ${e.message}`);
  }
  const rings = calls.slice(before).filter((c) => c[0] === "createRadialGradient").length;
  check("a ring near a corner is drawn across the wrap", drew && rings === 4, `${rings} rings`);

  const named = shell.seatLabels(
    { seats: [{ seat: 0, weights_hash: "sha256:0123456789abcdef" }, { seat: 1, label: "dense" }] },
    2,
    [{ seat: 0, name: "mover", by: "@someone" }]
  );
  check("the host's name for a seat wins", named[0].name === "mover" && named[0].by === "@someone");
  check("the envelope's stands where the host says nothing", named[1].name === "dense" && named[1].by === "");
  check(
    "and a hash is the last resort",
    shell.seatLabels({ seats: [{ seat: 0, weights_hash: "sha256:0123456789abcdef" }] }, 1)[0].name === "01234567"
  );

  // The seats' layout at the widths the viewer is given: the web's home page (505 pixels), its
  // match page (1112) and a phone (352).
  const columns = shell.seatColumns;
  check("two seats are one row on the home page", columns(2, 505) === 2);
  check("four there are two rows of two", columns(4, 505) === 2);
  check("six there are two rows of three, not four over two", columns(6, 505) === 3);
  check("six on the match page are one row", columns(6, 1112) === 6);
  check("six on a phone are three rows of two", columns(6, 352) === 2);
  let roomy = true;
  for (const n of [1, 2, 3, 4, 5, 6, 8]) {
    for (const w of [240, 352, 505, 640, 800, 1112]) {
      const c = columns(n, w);
      if (c < 1 || c > n || (c > 1 && w / c < 160)) roomy = false;
    }
  }
  check("no seat is laid out narrower than a seat can be", roomy);
}

console.log(failures ? `\n${failures} FAILURE(S)` : "\nall geometry checks passed");
process.exit(failures ? 1 : 0);
