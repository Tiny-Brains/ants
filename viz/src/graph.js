// The ants graph: the match as a line a seat.
//
// A player shows one turn at a time, and the story of a match -- who grew, who stalled, whose hills
// fell and when -- is spread across a thousand of them. This draws it at once: each seat's ants
// alive, hills standing or score over the whole match, one thin line a seat in its colour, behind a
// three-way switch. A playhead follows the viewer, and hovering scrubs it, so the graph is a second
// timeline with the story drawn on it. The ticks the timeline carries for a hill razed or a colony
// wiped out sit here on the line of the seat that lost them.
//
// **It reads the viewer and nothing else.** `series()`, `events`, `range`, `turn`, `seek()`,
// `on()` and `trackBox()` are the viewer's public surface, counted from the frames the viewer
// already decoded: no second decode, and no rule known here.
//
// **Its x-axis is the timeline's.** The graph sits under the player at the same width, and
// `trackBox()` says where the player's track is, so a turn is at the same x in both and a tick on
// the graph lines up with the same tick on the timeline. When the track moves -- a resize, a phone's
// layout putting it on a row of its own -- the viewer says `"layout"` and the plot is redrawn.
//
// **A step series is drawn a little apart.** Hills and score change in whole steps, and two seats
// level on the same value would draw one line, so a seat could vanish from the graph for as long as
// it was level with a rival. Each seat's step line is moved a fixed `(seat − (n − 1) / 2) × 1.5`
// pixels: the values stay exact, the lines stay apart, and a lane per seat would have spent the
// 140 pixels on empty space. Ants alive is a curve that is almost never level, and is not moved.
//
// Loaded by `mountGraph` on first use, not with `viz.js`, and styled by the viewer's one stylesheet
// (shell.js, scoped to `.tb-viz` and checked by check.mjs).

import { SEATS } from "./render.js";
import { injectCss } from "./shell.js";

/** The three series, in the switch's order; ants alive is the default. */
export const KINDS = [
  ["ants", "Ants"],
  ["hills", "Hills"],
  ["score", "Score"],
];
const HEIGHT = 140;
/** How far apart two seats level on a step series are drawn, in pixels. */
const STEP_PX = 1.5;
const TOP = 10;
const BOTTOM = 16;
/** Where the plot runs when the viewer has no track the graph can line up with. */
const GUTTER = { left: 34, right: 10 };

/** The fixed vertical offset of seat `seat`'s line on a step series of `n` seats, in pixels. */
export function stepOffset(seat, n) {
  return (seat - (n - 1) / 2) * STEP_PX;
}

/**
 * Where the plot runs across a canvas `width` pixels wide whose left edge is at `left` on the page:
 * under the viewer's track (`box`, from `trackBox()`), or inside the canvas's own gutters when the
 * track is somewhere the graph cannot reach -- a graph mounted in another column, or a viewer that
 * has no track.
 *
 * @returns {{ x0: number, w: number }}  the plot's left edge in the canvas, and its width
 */
export function plotSpan(box, left, width) {
  const own = { x0: GUTTER.left, w: Math.max(1, width - GUTTER.left - GUTTER.right) };
  if (!box || !(box.width >= 40)) return own;
  const x0 = box.left - left;
  if (x0 < 0 || x0 + box.width > width + 0.5) return own;
  return { x0, w: box.width };
}

export class Graph {
  /**
   * @param {HTMLElement} el   where to draw
   * @param {Viewer} viewer    the stage or player it follows
   * @param {object} [opts]    { kind, height, theme }
   */
  constructor(el, viewer, opts = {}) {
    this.el = el;
    this.viewer = viewer;
    this.opts = opts;
    this.kind = KINDS.some(([k]) => k === opts.kind) ? opts.kind : "ants";
    this.height = Number(opts.height) || HEIGHT;
    this.destroyed = false;

    injectCss(el.ownerDocument);
    el.innerHTML = "";
    el.classList.add("tb-viz");
    el.dataset.tbMode = "graph";
    if (opts.theme) el.dataset.tbTheme = opts.theme;

    const d = el.ownerDocument;
    const mk = (tag, cls, parent) => {
      const n = d.createElement(tag);
      if (cls) n.className = cls;
      (parent || el).appendChild(n);
      return n;
    };

    const head = mk("div", "tb-g-head");
    const seg = mk("div", "tb-seg", head);
    seg.setAttribute("role", "group");
    seg.setAttribute("aria-label", "Series");
    this.switches = KINDS.map(([k, label]) => {
      const b = mk("button", null, seg);
      b.type = "button";
      b.textContent = label;
      b.dataset.kind = k;
      b.setAttribute("aria-pressed", String(k === this.kind));
      b.onclick = () => this.setKind(k);
      return b;
    });
    this.label = mk("span", "tb-g-lab", head);

    const plot = mk("div", "tb-g-plot");
    this.plotEl = plot;
    this.canvas = mk("canvas", null, plot);
    this.canvas.setAttribute("role", "img");
    this.ctx = this.canvas.getContext("2d");
    this.layer = d.createElement("canvas");

    const legend = mk("div", "tb-legend");
    (viewer.labels ?? []).forEach(({ name, by }, seat) => {
      const s = mk("span", null, legend);
      s.style.setProperty("--tb-seat", SEATS[seat % SEATS.length]);
      mk("i", null, s);
      mk("b", null, s).textContent = name;
      if (by) s.appendChild(d.createTextNode(` ${by}`));
    });

    // Hovering scrubs. A match that was playing pauses while the pointer is over the graph and
    // plays on when it leaves, so the reader's hand is not fighting the clock for the playhead.
    const scrub = (e) => {
      const t = this.turnAt(e.clientX);
      if (t == null) return;
      if (viewer.playing && typeof viewer.pause === "function") {
        viewer.pause();
        this.resume = true;
      }
      viewer.seek(t);
    };
    const leave = () => {
      if (this.resume && typeof viewer.play === "function") viewer.play();
      this.resume = false;
    };
    this.canvas.addEventListener("pointermove", scrub);
    this.canvas.addEventListener("pointerdown", scrub);
    this.canvas.addEventListener("pointerleave", leave);
    this.canvas.addEventListener("pointerup", (e) => {
      if (e.pointerType === "touch") leave();
    });

    this.off = [viewer.on("turn", () => this.draw()), viewer.on("layout", () => this.plot())];
    this.ro = new ResizeObserver(() => this.plot());
    this.ro.observe(el);
    this.plot();
  }

  setKind(kind) {
    if (!KINDS.some(([k]) => k === kind) || kind === this.kind) return;
    this.kind = kind;
    for (const b of this.switches) b.setAttribute("aria-pressed", String(b.dataset.kind === kind));
    this.plot();
  }

  /** The turn under a pointer at page-x `clientX`, or null off the plot. */
  turnAt(clientX) {
    if (!this.span) return null;
    const r = this.canvas.getBoundingClientRect();
    const { lo, hi } = this.viewer.range;
    const f = Math.max(0, Math.min(1, (clientX - r.left - this.span.x0) / this.span.w));
    return Math.round(lo + f * (hi - lo));
  }

  /** x of turn `t` in the canvas. */
  x(t) {
    const { lo, hi } = this.viewer.range;
    return this.span.x0 + ((t - lo) / Math.max(1, hi - lo)) * this.span.w;
  }

  /** y of value `v` in the canvas. */
  y(v) {
    return TOP + (1 - v / this.top) * (this.height - TOP - BOTTOM);
  }

  /**
   * Draw everything that does not move with the turn -- grid, lines, ticks -- into a layer at the
   * canvas's size, once per series, size or layout. A turn then costs a blit and a playhead.
   */
  plot() {
    if (this.destroyed) return;
    const width = this.el.clientWidth;
    if (!(width >= 2)) return;
    const dpr = window.devicePixelRatio || 1;
    const H = this.height;
    for (const c of [this.canvas, this.layer]) {
      c.width = Math.max(1, Math.round(width * dpr));
      c.height = Math.max(1, Math.round(H * dpr));
    }
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${H}px`;
    this.width = width;

    const r = this.canvas.getBoundingClientRect();
    const view = this.el.ownerDocument.defaultView;
    this.span = plotSpan(this.viewer.trackBox?.(), r.left + (view?.scrollX ?? 0), width);

    const all = this.viewer.series()[this.kind] ?? [];
    const { lo, hi } = this.viewer.range;
    let most = 0;
    for (const line of all) for (let t = lo; t <= hi; t++) most = Math.max(most, line[t] ?? 0);
    // A curve gets headroom so its peak is not drawn on the frame; a step series tops out on a
    // whole value, where its gridline is.
    this.top = this.kind === "ants" ? Math.max(1, Math.ceil(most * 1.08)) : Math.max(1, most);

    const colour = themeColours(this.el);
    this.colour = colour;
    const g = this.layer.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, H);

    // Three gridlines, their values in the gutter left of the plot when there is room for them.
    const { x0, w } = this.span;
    g.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
    g.textBaseline = "middle";
    const inside = x0 < 26;
    g.textAlign = inside ? "left" : "right";
    for (const v of gridValues(this.top, this.kind)) {
      const y = Math.round(this.y(v)) + 0.5;
      g.strokeStyle = colour.line;
      g.lineWidth = 1;
      g.beginPath();
      g.moveTo(x0, y);
      g.lineTo(x0 + w, y);
      g.stroke();
      g.fillStyle = colour.dim;
      g.fillText(String(v), inside ? x0 + 3 : x0 - 6, inside ? y - 6 : y);
    }
    g.textBaseline = "alphabetic";
    g.textAlign = "left";
    g.fillText(String(lo), x0, H - 3);
    g.textAlign = "right";
    g.fillText(String(hi), x0 + w, H - 3);

    // The lines. A step series goes along and then up, never diagonally: a hill falls on a turn,
    // not across two.
    const n = all.length;
    const step = this.kind !== "ants";
    g.lineWidth = 1.5;
    g.lineJoin = "round";
    all.forEach((line, seat) => {
      const off = step ? stepOffset(seat, n) : 0;
      g.strokeStyle = SEATS[seat % SEATS.length];
      g.beginPath();
      g.moveTo(this.x(lo), this.y(line[lo] ?? 0) + off);
      for (let t = lo + 1; t <= hi; t++) {
        if (step) {
          if (line[t] === line[t - 1]) continue;
          g.lineTo(this.x(t), this.y(line[t - 1]) + off);
        }
        g.lineTo(this.x(t), this.y(line[t]) + off);
      }
      if (step) g.lineTo(this.x(hi), this.y(line[hi] ?? 0) + off);
      g.stroke();
    });

    // A hill razed and a colony wiped out, on the line of the seat that lost it, at the value its
    // line has on that turn.
    for (const ev of this.viewer.events ?? []) {
      const line = all[ev.seat];
      if (!line || ev.i < lo || ev.i > hi) continue;
      const x = Math.round(this.x(ev.i)) + 0.5;
      const y = this.y(line[ev.i] ?? 0) + (step ? stepOffset(ev.seat, n) : 0);
      const half = ev.kind === "wiped" ? 8 : 6;
      g.strokeStyle = SEATS[ev.seat % SEATS.length];
      g.lineWidth = ev.kind === "wiped" ? 2.5 : 2;
      g.beginPath();
      g.moveTo(x, y - half);
      g.lineTo(x, y + half);
      g.stroke();
    }

    this.draw();
  }

  /** The layer, the playhead at the turn shown, and the readout. Once a turn. */
  draw() {
    if (this.destroyed || !this.span) return;
    const dpr = window.devicePixelRatio || 1;
    const { ctx } = this;
    const H = this.height;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.drawImage(this.layer, 0, 0);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const t = this.viewer.turn;
    const x = Math.round(this.x(t)) + 0.5;
    ctx.strokeStyle = this.colour.ink;
    ctx.fillStyle = this.colour.ink;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(x, TOP - 4);
    ctx.lineTo(x, H - BOTTOM);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(x - 4, TOP - 6);
    ctx.lineTo(x + 4, TOP - 6);
    ctx.lineTo(x, TOP - 1);
    ctx.closePath();
    ctx.fill();

    const all = this.viewer.series()[this.kind] ?? [];
    const names = (this.viewer.labels ?? []).map((l) => l.name);
    const values = all.map((line, seat) => `${names[seat] ?? `seat ${seat}`} ${line[t] ?? 0}`);
    const text = `turn ${t} · ${values.join(" · ")}`;
    this.label.textContent = text;
    this.label.title = text;
    this.canvas.setAttribute("aria-label", `${this.kind} per seat. ${text}`);
  }

  destroy() {
    this.destroyed = true;
    for (const off of this.off) off();
    if (this.ro) this.ro.disconnect();
    this.el.innerHTML = "";
    this.el.classList.remove("tb-viz");
    delete this.el.dataset.tbMode;
  }
}

/** Gridlines at zero, the middle and the top, as whole numbers, without repeating one. */
function gridValues(top, kind) {
  const mid = kind === "ants" ? Math.round(top / 2) : Math.floor(top / 2);
  return [...new Set([0, mid, top])];
}

/**
 * The page's colours for the grid, the labels and the playhead, as the viewer's stylesheet
 * resolved them on this element: a canvas cannot read a CSS variable, so it is asked for the
 * computed value, which follows the host's tokens and theme. Literal fallbacks for a document that
 * computes nothing.
 */
function themeColours(el) {
  const view = el.ownerDocument.defaultView;
  const cs = view?.getComputedStyle ? view.getComputedStyle(el) : null;
  const read = (name, fallback) => (cs?.getPropertyValue(name) || "").trim() || fallback;
  return {
    line: read("--tb-line", "#B9C9E1"),
    dim: read("--tb-dim", "#536780"),
    ink: read("--tb-ink", "#142642"),
  };
}
