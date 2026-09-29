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

import { readFileSync, readdirSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

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

// ---------------------------------------------------------------- a document that keeps what it is told
//
// For the tiers: elements that keep their children, classes, attributes, data, styles and
// listeners, canvases whose context records every call, ResizeObservers that fire when told to and
// animation frames that run when told to. An element is as big as its nearest ancestor that was
// given a `rect`, and a host is given one.
function fakeDom() {
  const calls = [];
  const observers = [];
  let frames = [];
  let nextFrame = 1;
  const ctx = () =>
    new Proxy(
      {
        createImageData: (w, h) => ({ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }),
        createRadialGradient: () => ({ addColorStop() {} }),
      },
      {
        get(t, k) {
          if (k in t) return t[k];
          return (...args) => calls.push([k, ...args]);
        },
        set(t, k, v) {
          t[k] = v;
          return true;
        },
      }
    );
  class Node {
    constructor(doc, tag) {
      this.ownerDocument = doc;
      this.tagName = tag.toUpperCase();
      this.children = [];
      this.parentNode = null;
      this.className = "";
      this.dataset = {};
      this.attrs = {};
      this.listeners = {};
      this.style = { setProperty(k, v) { this[k] = String(v); }, removeProperty(k) { delete this[k]; } };
      this.hidden = false;
      this.textContent = "";
      this.html = "";
      if (tag === "canvas") {
        this.width = 0;
        this.height = 0;
        const c = ctx();
        this.getContext = () => c;
      }
    }
    get classList() {
      const names = () => this.className.split(/\s+/).filter(Boolean);
      return {
        contains: (c) => names().includes(c),
        add: (...cs) => (this.className = [...new Set([...names(), ...cs])].join(" ")),
        remove: (...cs) => (this.className = names().filter((n) => !cs.includes(n)).join(" ")),
      };
    }
    set innerHTML(v) {
      this.html = String(v);
      for (const c of this.children) c.parentNode = null;
      this.children = [];
    }
    get innerHTML() {
      return this.html;
    }
    appendChild(n) {
      return this.insertBefore(n, null);
    }
    insertBefore(n, ref) {
      if (n.parentNode) n.parentNode.children = n.parentNode.children.filter((c) => c !== n);
      const at = ref ? this.children.indexOf(ref) : -1;
      if (at < 0) this.children.push(n);
      else this.children.splice(at, 0, n);
      n.parentNode = this;
      return n;
    }
    setAttribute(k, v) {
      this.attrs[k] = String(v);
    }
    getAttribute(k) {
      return this.attrs[k] ?? null;
    }
    removeAttribute(k) {
      delete this.attrs[k];
    }
    addEventListener(type, fn) {
      (this.listeners[type] ??= []).push(fn);
    }
    removeEventListener(type, fn) {
      this.listeners[type] = (this.listeners[type] ?? []).filter((f) => f !== fn);
    }
    fire(type, ev = {}) {
      for (const fn of this.listeners[type] ?? []) fn({ pointerId: 1, pointerType: "mouse", preventDefault() {}, ...ev });
    }
    setPointerCapture() {}
    releasePointerCapture() {}
    getBoundingClientRect() {
      for (let n = this; n; n = n.parentNode) if (n.rect) return { ...n.rect };
      return { left: 0, top: 0, width: 0, height: 0 };
    }
    get clientWidth() {
      return this._cw ?? this.getBoundingClientRect().width;
    }
  }
  const window = { devicePixelRatio: 1, scrollX: 0, scrollY: 0, getComputedStyle: () => ({ getPropertyValue: () => "" }) };
  const document = new Node(null, "#document");
  document.ownerDocument = null;
  document.defaultView = window;
  document.fullscreenElement = null;
  document.head = new Node(document, "head");
  document.createElement = (tag) => new Node(document, tag);
  document.createTextNode = (text) => Object.assign(new Node(document, "#text"), { textContent: text });
  document.getElementById = (id) => document.head.children.find((c) => c.id === id) ?? null;
  globalThis.document = document;
  globalThis.window = window;
  globalThis.ResizeObserver = class {
    constructor(cb) {
      this.cb = cb;
      this.live = true;
      observers.push(this);
    }
    observe() {}
    disconnect() {
      this.live = false;
    }
  };
  globalThis.requestAnimationFrame = (cb) => {
    frames.push({ id: nextFrame, cb });
    return nextFrame++;
  };
  globalThis.cancelAnimationFrame = (id) => {
    frames = frames.filter((f) => f.id !== id);
  };
  return {
    calls,
    window,
    document,
    host(width = 1000, height = 700) {
      const el = document.createElement("div");
      el.rect = { left: 0, top: 0, width, height };
      el._cw = width;
      return el;
    },
    /** Fire every live ResizeObserver, as a resize would. */
    resize() {
      for (const o of observers) if (o.live) o.cb([]);
    },
    /** Run the queued animation frames, each a long way after the last, until none is queued. */
    runFrames() {
      for (let k = 0; k < 1000 && frames.length; k++) {
        const due = frames;
        frames = [];
        for (const f of due) f.cb(performance.now() + 1e7);
      }
    },
  };
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
const replay = JSON.parse(readFileSync("../engine/src/tests/fixtures/replay-maze-03.json", "utf8"));

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
// From ../dist/viz/, because that is where the transpiled component lives -- src/engine.js imports
// it by a path that only exists after build.sh has run.
const { allFrames } = await import("../dist/viz/engine.js");
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
  const shell = await import("../dist/viz/shell.js");
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
check("the tray exists", /"tb-tray\b/.test(mod) && /\.tb-viz \.tb-tray\{/.test(css));
check("it is hidden until wanted", /\.tb-viz \.tb-tray\{[^}]*opacity:0/.test(css));
check(
  "hover, focus and touch raise it",
  /:hover/.test(css) && /:focus-within/.test(css) && /\[data-tb-peek\]/.test(css) && /pointerType === "touch"/.test(mod)
);
check("the transport is not in it", /make\("div", "tb-bar"\)/.test(mod));
// The seats are the other half of what is always on screen: who is playing and the score are read
// the whole way through, so they sit in a bar of their own above the board rather than in the tray.
check(
  "the seats are a bar of their own, always on screen",
  /this\.seats = this\.make\("div", "tb-top"\);/.test(mod) &&
    /\.tb-viz \.tb-top\{/.test(css) &&
    !/\.tb-top\{[^}]*opacity/.test(css)
);
// One card per seat: its colour the card's left edge, name and owner on top, the score large.
check(
  "each seat is a card: its colour, name and score",
  /\.tb-viz \.tb-seat\{[^}]*border-left:3px solid var\(--tb-seat\)/.test(css) &&
    /"tb-name"/.test(mod) &&
    /"tb-score"/.test(mod) &&
    /\.tb-viz \.tb-score\{[^}]*font:650 22px/.test(css)
);
check("the board's label and the cell readout are gone", !/tb-tip|tb-meta|showTip/.test(mod));
check("seat cards are built once, not per frame", /buildSeats\(\)/.test(mod) && !/this\.seats\.innerHTML = ""/.test(mod));
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
  const shell = await import("../dist/viz/shell.js");

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

  // What a turn did, from the frame's own record: a death is a ring with a cross and a line from
  // each killer, two that killed each other meeting halfway; a razed hill is a dashed square. A
  // frame from an engine that wrote none of this -- a stored last frame with three-element ants --
  // draws as it always did.
  const lineCount = (from) => calls.slice(from).filter((c) => c[0] === "lineTo").length;
  const small = { rows: 8, cols: 12, water: [0, 96] };
  const rm = new Renderer(makeStubCanvas());
  rm.setBoard(small);
  rm.resize(600, 400); // fifty pixels a cell: every mark is drawn
  const quiet = {
    turn: 3, size: [8, 12], water: { rle: [0, 96] }, food: [], hills: [[1, 1, 0], [5, 7, 1]],
    ants: [[3, 2, 0, 0], [3, 6, 1, 0]], deaths: [], razed: [], score: [1, 1], ranks: [1, 1], done: false,
  };
  const fought = {
    ...quiet, turn: 4, ants: [], score: [1, 1],
    deaths: [
      { ant: [3, 3, 0, 0], by: [[3, 5, 1, 0]] },
      { ant: [3, 5, 1, 0], by: [[3, 3, 0, 0]] },
    ],
  };
  let mark = calls.length;
  rm.render(quiet, {});
  const still = lineCount(mark);
  mark = calls.length;
  rm.render(fought, {});
  const fights = calls.slice(mark);
  const arcs = fights.filter((c) => c[0] === "arc").length;
  const heads = fights.filter((c) => c[0] === "closePath").length;
  check(
    "a death is a ring on the square it died on, with a line from its killer",
    arcs >= 2 && lineCount(mark) > still,
    `${arcs} rings, ${lineCount(mark)} lines`
  );
  check("two ants that killed each other draw two lines that meet halfway, with no head", heads === 0, `${heads} heads`);
  mark = calls.length;
  rm.render({ ...quiet, turn: 4, ants: [[3, 2, 0, 0]], deaths: [{ ant: [3, 4, 1, 0], by: [[3, 2, 0, 0]] }] }, {});
  const oneWay = calls.slice(mark).filter((c) => c[0] === "closePath").length;
  check("a one-way kill is headed at the victim", oneWay === 1, `${oneWay} heads`);
  mark = calls.length;
  rm.render({ ...quiet, turn: 5, hills: [[1, 1, 0]], razed: [[5, 7, 1, 0]] }, {});
  const dashed = calls.slice(mark).filter((c) => c[0] === "setLineDash" && c[1].length === 2).length;
  check("a razed hill is a dashed square where it stood", dashed >= 1 && lineCount(mark) > still);
  mark = calls.length;
  let old = true;
  try {
    rm.render({ ...quiet, ants: [[3, 2, 0], [3, 6, 1]], deaths: undefined, razed: undefined }, {});
  } catch (e) {
    old = false;
    console.log(`        ${e.message}`);
  }
  check("a frame with no ids and no record draws as before", old && lineCount(mark) === still);
  const said = shell.describeEvents(fought, ["ember", "azure"]);
  check(
    "the turn's deaths are said for a screen reader",
    said.length === 2 && said[0] === "ember lost 1 ant to azure" && said[1] === "azure lost 1 ant to ember",
    JSON.stringify(said)
  );
  const bump = shell.describeEvents(
    { deaths: [{ ant: [1, 1, 0, 2], by: [] }, { ant: [1, 1, 0, 3], by: [] }], razed: [[5, 7, 1, 0]] },
    ["ember", "azure"]
  );
  check(
    "a collision and a razing are said too",
    bump[0] === "ember lost 2 ants in a collision" && bump[1] === "azure's hill razed by ember",
    JSON.stringify(bump)
  );

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

  // The seat cards' layout at the widths the viewer is given: a phone (352), the web's home page
  // (505), a model page (800), its match page (1112) and a theatre (1400).
  const columns = shell.seatColumns;
  check("two seats are one row on the home page", columns(2, 505) === 2);
  check("four there are two rows of two", columns(4, 505) === 2);
  check("four on a model page are one row", columns(4, 800) === 4);
  check("six on the match page are two rows of three, not four over two", columns(6, 1112) === 3);
  check("six on a phone are three rows of two", columns(6, 352) === 2);
  let wraps = true;
  let why = "";
  for (const n of [5, 6, 7, 8]) {
    for (const w of [640, 800, 1112, 1400, 2400]) {
      const c = columns(n, w);
      if (Math.ceil(n / c) !== 2) {
        wraps = false;
        why = `${n} seats at ${w}px: ${c} columns`;
      }
    }
  }
  check("five to eight seats wrap onto two rows at any width from 640", wraps, why);
  let pairs = true;
  for (const n of [2, 3, 4, 5, 6, 7, 8]) for (const w of [240, 352, 505, 639]) if (columns(n, w) !== 2) pairs = false;
  check("below 640 pixels the cards go two to a row", pairs);
  let roomy = true;
  for (const n of [1, 2, 3, 4, 5, 6, 8]) {
    for (const w of [640, 800, 1112, 1400]) {
      const c = columns(n, w);
      if (c < 1 || c > n || (c > 1 && w / c < 160)) roomy = false;
    }
  }
  check("no card is laid out narrower than a card can be", roomy);
}

// ---------------------------------------------------------------- the map visual
//
// A board on its own, for a season's map page and the book's board pages: map.js. What can be
// checked without eyes is its arithmetic, that the board it draws is the cartridge's turn zero --
// every hill the file lists, owned as the engine says -- and that it has nothing to operate.
console.log("map visual");
{
  const { mapFrame } = await import("../dist/viz/map.js");
  const eq = (a, b) => a.w === b.w && a.h === b.h;
  check("a square board is as tall as it is wide", eq(mapFrame(360, 36, 36), { w: 360, h: 360 }));
  check("a wide board is shorter than its width", eq(mapFrame(372, 120, 124), { w: 372, h: 360 }));
  check("a tall board stops at the host's cap", eq(mapFrame(300, 96, 80, 280), { w: 300, h: 280 }));
  check("a host still measuring zero draws a pixel, not NaN", eq(mapFrame(0, 24, 24), { w: 1, h: 1 }));
  check("a board with no size draws nothing", eq(mapFrame(300, 0, 0), { w: 300, h: 1 }));

  const { frameAt } = await import("../dist/viz/engine.js");
  let whole = true;
  let why = "";
  for (const f of readdirSync("../maps").filter((n) => n.endsWith(".json"))) {
    const board = JSON.parse(readFileSync(`../maps/${f}`, "utf8"));
    const frame = frameAt({ seed: 1, max_turns: 1, turns: 0, map: board, deltas: [] }, 0);
    const owners = new Set(frame.hills.map((h) => h[2]));
    if (frame.hills.length !== board.hills.length || owners.size !== board.players) {
      whole = false;
      why = `${board.id}: ${frame.hills.length} hills, ${owners.size} owners`;
    }
  }
  check("every basic board's turn zero is every hill, every seat owning some", whole, why);

  const src = readFileSync("./src/map.js", "utf8");
  check(
    "the map visual has nothing to operate",
    !/createElement\("button"|addEventListener|tabIndex|\bonclick\b/.test(src)
  );
}

// ---------------------------------------------------------------- the tiers
//
// What each tier builds, and what it does with the frames it is given, need a document -- but not a
// browser: a page's worth of elements that keep their children, classes, attributes and listeners,
// and canvases that count what is drawn on them. Layout is the one thing faked outright: an element
// is as big as its nearest ancestor that was given a size, which is how each host below is sized.
console.log("tiers");
const fx = fakeDom();
const shellT = await import("../dist/viz/shell.js");
const vizT = await import("../dist/viz/viz.js");
const { Viewer } = shellT;
const find = (root, cls) => {
  const out = [];
  const walk = (n) => {
    for (const c of n.children) {
      if (c.classList.contains(cls)) out.push(c);
      walk(c);
    }
  };
  walk(root);
  return out;
};
const has = (v, cls) => find(v.el, cls).length > 0;
const board = (id) => JSON.parse(readFileSync(`../maps/${id}.json`, "utf8"));
const unplayed = (map) => ({ seed: 1, max_turns: 1, turns: 0, map, deltas: [] });
const lastFrame = JSON.parse(readFileSync("./last-frame-basic-xlarge-8p.json", "utf8"));
// The stored frame is what Soma keeps for a match and what a tile or a thumb draws without
// decoding anything, so it has to be the shape THIS engine emits. It sat two digests behind --
// three-element ants, no `deaths`, no `razed` -- because `make-last-frame.mjs` wrote the digest it
// played on and nothing ever read it back. This is what makes writing it mean something.
//
// THE COMPARISON IS THE SHAPE, NOT THE DIGEST. rustc's host is part of the component's bytes
// (build.yml says so at the top), so the fixture's `engine_digest` is whatever machine last ran
// `make-last-frame.mjs` and can never equal a CI build's -- asserting equality fails every run on
// the arm64 Linux runner, and nothing in CI regenerates the fixture. What actually went stale was
// the SHAPE, and that is checkable against a frame this build just produced: same keys, same ant
// arity. `engine_digest` stays in the file as provenance for whoever regenerates it.
const liveFrame = frames[frames.length - 1];
const shapeOf = (f) => `${Object.keys(f).sort().join(" ")} | ants[${f.ants?.[0]?.length ?? 0}]`;
check(
  "the stored last frame is the shape this engine emits",
  shapeOf(lastFrame.frame) === shapeOf(liveFrame),
  `fixture ${shapeOf(lastFrame.frame)}, engine ${shapeOf(liveFrame)}; regenerate with \`node make-last-frame.mjs\``
);
const labels8 = lastFrame.seats.map((s) => ({ seat: s.seat, name: s.name }));
const labels2 = [
  { seat: 0, name: "left-model", by: "@one" },
  { seat: 1, name: "right-model", by: "@two" },
];

{
  const stage = new Viewer(fx.host(1000, 700), replay, {});
  check("a stage is the default tier", stage.tier === "stage" && stage.el.classList.contains("tb-tier-stage"));
  check(
    "a stage builds seat cards, the tray, the transport, speed, the key list and fullscreen",
    ["tb-top", "tb-seat", "tb-tray", "tb-bar", "tb-track", "tb-speed", "tb-keys-btn", "tb-keys", "tb-full"].every((c) =>
      has(stage, c)
    ) && stage.el.tabIndex === 0
  );
  check("and nothing a tile or a thumb has", !has(stage, "tb-over") && !has(stage, "tb-chips"));
  check("its cards carry the counts line", find(stage.el, "tb-nums").length === 2 && has(stage, "tb-hill"));

  const player = new Viewer(fx.host(720, 520), replay, { tier: "player" });
  check(
    "a player builds cards, the transport and fullscreen",
    ["tb-top", "tb-bar", "tb-track", "tb-turn", "tb-full"].every((c) => has(player, c)) &&
      [player.firstBtn, player.prevBtn, player.playBtn, player.nextBtn, player.lastBtn].every(Boolean)
  );
  check(
    "and no tray, speed or key list",
    !has(player, "tb-tray") && !has(player, "tb-speed") && !has(player, "tb-keys-btn") && !has(player, "tb-keys")
  );
  const scale = player.renderer.scale;
  player.zoom(2);
  player.key({ key: "+", preventDefault() {} });
  check("a player does not zoom", player.renderer.scale === scale);
  // A lesson board six rows by eight: the eight-move reach covers it, so no hill is ringed.
  {
    const tiny = {
      ...replay,
      turns: 0,
      map: { id: "tiny", rows: 6, cols: 8, players: 2, water: [0, 48], hills: [[1, 1], [4, 5]], food: [], food_target: 0, symmetry: { dr: 3, dc: 4 } },
      deltas: [],
    };
    const small = new shellT.Viewer(fx.host(600, 400), tiny, { tier: "stage" });
    check("a ring that would cover the board is not drawn", small.threatsAt(0).length === 0, `${small.threatsAt(0).length} rings`);
    small.destroy();
  }
  check("a player's cards drop the counts line", /\.tb-viz\.tb-tier-player \.tb-nums\{display:none\}/.test(css));

  const tile = new Viewer(fx.host(320, 200), null, { tier: "tile", frame: frames.at(-1), labels: labels2 });
  check(
    "a tile builds the board and its overlay, and nothing to operate",
    has(tile, "tb-stage") &&
      has(tile, "tb-over") &&
      ["tb-top", "tb-bar", "tb-tray", "tb-full"].every((c) => !has(tile, c)) &&
      tile.el.tabIndex === undefined &&
      Object.keys(tile.el.listeners).length === 0
  );
  const thumb = vizT.drawFrame(fx.host(112, 70), frames.at(-1));
  check(
    "a thumb is the canvas and a chip",
    thumb.tier === "thumb" &&
      has(thumb, "tb-stage") &&
      has(thumb, "tb-chips") &&
      ["tb-top", "tb-bar", "tb-tray", "tb-over"].every((c) => !has(thumb, c)) &&
      thumb.el.tabIndex === undefined
  );
  check("a tile and a thumb are a 16:10 box", /\.tb-viz:is\(\.tb-tier-tile,\.tb-tier-thumb\)\{[^}]*aspect-ratio:16 \/ 10/.test(css));
  let refused = 0;
  for (const [r, o] of [
    [null, { tier: "stage" }],
    [null, { tier: "player" }],
    [null, { tier: "tile", frame: frames.at(-1) }],
  ]) {
    try {
      new Viewer(fx.host(), r, o);
    } catch {
      refused++;
    }
  }
  check("a stage or player without a replay, or a tile without one or labels, is refused", refused === 3);
}

// Seat cards: one a seat, the columns from seatColumns(), two rows from five seats up, and the
// owners gone below 640 pixels. Each board's turn zero, so every seat count the envelope has.
{
  let laid = true;
  let why = "";
  for (const [id, n, cols] of [
    ["basic-small-3p", 3, 3],
    ["basic-medium-4p", 4, 4],
    ["basic-large-6p", 6, 3],
    ["basic-xlarge-8p", 8, 4],
  ]) {
    const v = new Viewer(fx.host(1200, 800), unplayed(board(id)), {});
    const got = Number(v.seats.style["--tb-cols"]);
    if (v.seatRows.length !== n || find(v.el, "tb-seat").length !== n || got !== cols) {
      laid = false;
      why = `${id}: ${v.seatRows.length} cards in ${got} columns`;
    }
    if (v.el.dataset.tbNarrow) laid = false;
    v.el._cw = 600;
    v.fit();
    if (Number(v.seats.style["--tb-cols"]) !== 2 || !v.el.dataset.tbNarrow) {
      laid = false;
      why = `${id} at 600px: ${v.seats.style["--tb-cols"]} columns, narrow ${v.el.dataset.tbNarrow}`;
    }
    v.el._cw = 1200;
    v.fit();
    if (v.el.dataset.tbNarrow) laid = false;
  }
  check("3, 4, 6 and 8 seats lay out as 3, 4, 3+3 and 4+4, and two a row below 640", laid, why);
  check("below 640 the owners drop", /\.tb-viz\[data-tb-narrow\] \.tb-by\{display:none\}/.test(css));

  const hills8 = shellT.hillsPerSeat(board("basic-xlarge-8p"), 8);
  check("each seat starts with its orbit's hills", hills8.every((h) => h === 2), JSON.stringify(hills8));
  const v = new Viewer(fx.host(1000, 700), replay, { turn: replay.turns });
  const f = frames.at(-1);
  let squares = true;
  v.seatRows.forEach((row, seat) => {
    const standing = f.hills.filter((h) => h[2] === seat).length;
    const filled = row.squares.filter((sq) => "on" in sq.dataset).length;
    if (row.squares.length !== shellT.hillsPerSeat(replay.map, 2)[seat] || filled !== standing) squares = false;
  });
  check("a card's hills are filled while they stand and hollow once razed", squares);
}

// The tile's overlay is a layer beside the canvas, never inside it: two seats in mirrored corners,
// three to eight the two leaders and how many more.
{
  const two = new Viewer(fx.host(320, 200), null, { tier: "tile", frame: frames.at(-1), labels: labels2 });
  const corners = find(two.el, "tb-corner");
  const f = frames.at(-1);
  check(
    "two seats sit in mirrored top corners",
    corners.length === 2 &&
      corners[0].classList.contains("tb-l") &&
      corners[1].classList.contains("tb-r") &&
      find(corners[0], "tb-nm")[0].textContent === "left-model" &&
      find(corners[1], "tb-sc")[0].textContent === String(f.score[1])
  );
  check(
    "the overlay is outside the drawing",
    two.overlay.parentNode === two.el && two.canvas.children.length === 0 && two.stage.children.includes(two.canvas)
  );
  check("the turn sits bottom right", two.turnTag.textContent === String(f.turn) && !two.turnTag.hidden);

  const eight = new Viewer(fx.host(400, 250), null, { tier: "tile", frame: lastFrame.frame, labels: labels8 });
  const score = lastFrame.frame.score;
  const lead = score
    .map((s, seat) => ({ s, seat }))
    .sort((a, b) => b.s - a.s || a.seat - b.seat)
    .slice(0, 2)
    .map((x) => labels8[x.seat].name);
  const names = find(eight.el, "tb-nm").map((n) => n.textContent);
  check(
    "eight seats show the two leaders by score and +6",
    find(eight.el, "tb-list").length === 1 &&
      names.join() === lead.join() &&
      find(eight.el, "tb-more")[0]?.textContent === "+6",
    `${names} vs ${lead}`
  );

  const queued = new Viewer(fx.host(320, 200), null, { tier: "tile", board: board("basic-tiny-2p"), labels: labels2 });
  check(
    "a board nobody has played shows turn zero with no scores and no turn",
    queued.turn === 0 && find(queued.el, "tb-sc").every((n) => n.textContent === "—") && queued.turnTag.hidden
  );
}

// drawFrame never calls the component. Proved by drawing with a copy of the viewer whose component
// throws on any call: a stored 8-seat last frame, a thousand turns in, drawn whole.
{
  const dir = mkdtempSync(join(tmpdir(), "tb-viz-stub-"));
  for (const f of readdirSync("../dist/viz").filter((n) => n.endsWith(".js"))) {
    writeFileSync(join(dir, f), readFileSync(`../dist/viz/${f}`));
  }
  mkdirSync(join(dir, "engine"));
  writeFileSync(
    join(dir, "engine/tb-ants.js"),
    `export const functions = { invoke() { globalThis.__tbComponentCalls = (globalThis.__tbComponentCalls ?? 0) + 1; throw new Error("the component was called"); } };\n`
  );
  const stubbed = await import(pathToFileURL(join(dir, "viz.js")).href);
  let live = false;
  try {
    stubbed.frameAt(replay, 1);
  } catch {
    live = true;
  }
  globalThis.__tbComponentCalls = 0;
  const f = lastFrame.frame;
  const before = fx.calls.length;
  const v = stubbed.drawFrame(fx.host(200, 125), f);
  const drawn = fx.calls.slice(before);
  const arcs = drawn.filter((c) => c[0] === "arc").length;
  const squares = drawn.filter((c) => c[0] === "strokeRect").length;
  check("the stubbed component refuses every call", live);
  check(
    "drawFrame draws a stored 8-seat last frame without calling the component",
    globalThis.__tbComponentCalls === 0 && !/tb-err/.test(v.el.innerHTML) && v.frames[0] === f,
    `${globalThis.__tbComponentCalls} calls`
  );
  check(
    "on the frame's own board",
    v.renderer.rows === f.size[0] && v.renderer.cols === f.size[1] && lastFrame.turn === 1000 && f.score.length === 8,
    `${v.renderer.rows}x${v.renderer.cols}`
  );
  check(
    "every ant, food and standing hill of it",
    arcs >= f.ants.length + f.food.length && squares >= f.hills.length + 1,
    `${arcs} arcs, ${squares} squares`
  );
  const water = shellT.boardOfFrame(f);
  check("the frame's water is the board's", water.water === f.water.rle && water.rows === 120 && water.cols === 124);
  const tile = stubbed.drawFrame(fx.host(320, 200), f, { tier: "tile", labels: labels8 });
  check("a tile given a frame goes the same way", tile.tier === "tile" && globalThis.__tbComponentCalls === 0);
  rmSync(dir, { recursive: true, force: true });

  // The chip, from 160 pixels up: whatever a ResizeObserver reports.
  const small = vizT.drawFrame(fx.host(112, 70), f);
  const wide = vizT.drawFrame(fx.host(200, 125), f);
  check("a thumb under 160 pixels has no chip", small.chips.hidden === true);
  check("from 160 it has one", wide.chips.hidden === false && find(wide.el, "tb-more")[0]?.textContent === "+6");
  wide.el._cw = 150;
  fx.resize();
  check("and loses it when it shrinks", wide.chips.hidden === true);
}

// A tile's preview: the last forty turns through the range form, played once and rested on the
// last, then stop() back to the frame it rests on. The ends: a match shorter than forty turns, and
// one that never left turn zero.
{
  check(
    "the preview range is the last forty turns, or all of fewer",
    [
      [1000, 960, 1000],
      [256, 216, 256],
      [41, 1, 41],
      [40, 0, 40],
      [25, 0, 25],
      [0, 0, 0],
    ].every(([t, a, b]) => {
      const [x, y] = shellT.previewRange(t);
      return x === a && y === b;
    })
  );
  const rest = frames.at(-1);
  const tile = new Viewer(fx.host(320, 200), null, { tier: "tile", frame: rest, labels: labels2 });
  const short = { ...replay, turns: 20, deltas: replay.deltas.slice(0, 20) };
  await tile.preview(short);
  check(
    "a match shorter than forty turns previews whole",
    tile.frames.length === 21 && tile.frames[0].turn === 0 && tile.frames.at(-1).turn === 20 && tile.playing
  );
  fx.runFrames();
  check("it plays once and rests on its last frame", !tile.playing && tile.turn === 20 && tile.i === tile.hi);
  tile.stop();
  check("stop() goes back to the resting frame", tile.frames[0] === rest && tile.turn === rest.turn);
  await tile.preview({ ...replay, turns: 0, deltas: [] });
  check("a match at turn zero previews one frame", tile.frames.length === 1 && tile.turn === 0);
  fx.runFrames();
  tile.stop();
  let fetches = 0;
  globalThis.fetch = async () => {
    fetches++;
    return { json: async () => replay };
  };
  await tile.preview("https://example.test/replay.json");
  check(
    "a whole match previews its last forty turns",
    tile.frames.length === 41 && tile.frames[0].turn === replay.turns - 40,
    `${tile.frames.length} frames from ${tile.frames[0].turn}`
  );
  fx.runFrames();
  tile.stop();
  await tile.preview("https://example.test/replay.json");
  check("and fetches its replay once", fetches === 1, `${fetches} fetches`);
  tile.stop();
  const pending = tile.preview("https://example.test/other.json");
  tile.stop();
  await pending;
  check("a preview stopped while it fetched never plays", !tile.playing && tile.frames[0] === rest);
  let refused = false;
  try {
    await new Viewer(fx.host(), replay, {}).preview(replay);
  } catch {
    refused = true;
  }
  check("only a tile previews", refused);
}

// The public surface the graph reads, and that series() is the frames' own counts.
{
  const v = new Viewer(fx.host(1000, 700), replay, {});
  const s = v.series();
  let same = s.ants.length === 2 && s.ants[0].length === frames.length;
  for (let t = 0; t < frames.length && same; t++) {
    for (let seat = 0; seat < 2; seat++) {
      const f = frames[t];
      if (
        s.ants[seat][t] !== f.ants.filter((a) => a[2] === seat).length ||
        s.hills[seat][t] !== f.hills.filter((h) => h[2] === seat).length ||
        s.score[seat][t] !== f.score[seat]
      ) {
        same = false;
      }
    }
  }
  check("series() is each frame's own counts, [seat][turn]", same);
  check("and is counted once", v.series() === s);
  check("range is the timeline's turns", v.range.lo === 0 && v.range.hi === replay.turns);
  const seen = [];
  const off = v.on("turn", (t) => seen.push(t));
  v.seek(50);
  check("seek() shows a turn and on('turn') hears it", v.turn === 50 && seen.at(-1) === 50);
  off();
  v.seek(60);
  check("and the unsubscribe it returned stops it", seen.at(-1) === 50 && v.turn === 60);
  check(
    "events say which seat lost what, and when",
    v.events.every((e) => (e.kind === "razed" || e.kind === "wiped") && e.turn === frames[e.i].turn && e.seat >= 0)
  );
  v.track.rect = { left: 180, top: 600, width: 500, height: 28 };
  fx.window.scrollX = 10;
  const boxes = [];
  v.on("layout", (b) => boxes.push(b));
  v.fit();
  fx.window.scrollX = 0;
  check(
    "trackBox() is the track in page pixels, and a layout re-emits it",
    boxes.length === 1 && boxes[0].left === 190 && boxes[0].width === 500
  );
}

// Fullscreen promotes a player to a stage in place: the same element, canvas and frames, with the
// stage's parts built on first use, and a player again after.
{
  const el = fx.host(720, 520);
  const v = new Viewer(el, replay, { tier: "player" });
  const canvas = v.canvas;
  const stageEl = v.stage;
  const seats = v.seatRows;
  v.fullscreen();
  check(
    "fullscreen makes a player a stage without a remount",
    v.tier === "stage" &&
      el.classList.contains("tb-tier-stage") &&
      !el.classList.contains("tb-tier-player") &&
      v.canvas === canvas &&
      v.stage === stageEl &&
      v.seatRows === seats &&
      el.children.includes(stageEl) &&
      has(v, "tb-tray") &&
      has(v, "tb-speed") &&
      el.dataset.tbFull === "1"
  );
  v.fullscreen(false);
  check(
    "and leaving it makes it a player again, the stage's parts put away",
    v.tier === "player" && el.classList.contains("tb-tier-player") && v.canvas === canvas && !el.dataset.tbFull &&
      find(el, "tb-tray")[0].classList.contains("tb-so") &&
      /\.tb-viz\.tb-tier-player \.tb-so\{display:none\}/.test(css)
  );
  // The Fullscreen API, where the browser has one.
  el.requestFullscreen = () => {
    fx.document.fullscreenElement = el;
    fx.document.fire("fullscreenchange");
    return Promise.resolve();
  };
  fx.document.exitFullscreen = () => {
    fx.document.fullscreenElement = null;
    fx.document.fire("fullscreenchange");
  };
  v.fullscreen();
  const up = v.tier === "stage" && v.isFull() && v.fullBtn.getAttribute("aria-pressed") === "true";
  v.fullscreen();
  check("the browser's own fullscreen does the same", up && v.tier === "player" && !v.isFull() && v.canvas === canvas);
  const stage = new Viewer(fx.host(), replay, {});
  stage.fullscreen();
  check("a stage stays a stage", stage.tier === "stage" && stage.isFull());
  stage.fullscreen(false);
}

// graph.js: loaded on first use, its x-axis the timeline's, one line a seat, hovering scrubs.
console.log("the ants graph");
{
  const index = readFileSync("./src/index.js", "utf8");
  check(
    "graph.js and map.js are loaded on first use, not with viz.js",
    !/^import[^;]*"\.\/(graph|map)\.js"/m.test(index) && /await import\("\.\/graph\.js"\)/.test(index)
  );
  const { stepOffset, plotSpan } = await import("../dist/viz/graph.js");
  check("two seats level on a step series draw 1.5 pixels apart", stepOffset(0, 2) === -0.75 && stepOffset(1, 2) === 0.75);
  const eight = Array.from({ length: 8 }, (_, s) => stepOffset(s, 8));
  check(
    "eight are spread evenly about the value",
    near(eight.reduce((a, b) => a + b, 0), 0) && near(eight[7] - eight[0], 7 * 1.5)
  );
  check("a graph with no track to follow keeps its own gutters", plotSpan(null, 0, 600).x0 === 34);

  const v = new Viewer(fx.host(1000, 700), replay, {});
  v.track.rect = { left: 180, top: 600, width: 600, height: 28 };
  const gh = fx.host(1000, 200);
  const g = await vizT.mountGraph(gh, v);
  check("its x-axis is the timeline's", g.span.x0 === 180 && g.span.w === 600, JSON.stringify(g.span));
  check(
    "a three-way switch, ants first, and a legend a seat",
    g.switches.length === 3 &&
      g.switches[0].getAttribute("aria-pressed") === "true" &&
      g.kind === "ants" &&
      find(gh, "tb-legend")[0].children.length === 2
  );
  g.canvas.fire("pointermove", { clientX: 180 + 300 });
  check("hovering scrubs the viewer", v.turn === Math.round(replay.turns / 2), `turn ${v.turn}`);
  v.seek(10);
  check("the playhead follows the viewer", g.label.textContent.startsWith("turn 10 "));
  g.setKind("hills");
  check("the switch changes the series", g.kind === "hills" && g.switches[1].getAttribute("aria-pressed") === "true");
  v.track.rect = { left: 220, top: 600, width: 520, height: 28 };
  v.fit();
  check("a layout moves its x-axis with the track", g.span.x0 === 220 && g.span.w === 520);
  const listening = v.listeners.turn.size;
  g.destroy();
  check("destroy() stops following", v.listeners.turn.size === listening - 1);
}

console.log(failures ? `\n${failures} FAILURE(S)` : "\nall viewer checks passed");
process.exit(failures ? 1 : 0);
