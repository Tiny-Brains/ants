// The player: everything around the pixels.
//
// It behaves like a media player because that is what watching a match is — transport buttons, a
// timeline you can click and drag, playback speed, and a keyboard that does what a keyboard does in
// a player. One thing it does that a video player cannot: the timeline is annotated, so a hill razed
// or a colony wiped out can be found without scrubbing for it. And two things are drawn over the
// board to say where to look: a ring around any hill an enemy is within eight moves of, and -- on a
// toggle -- the territory each seat has explored, from the engine's own record of what it saw.
//
// Three rules this file keeps.
//
// **Two bars and a board.** The seats are a title bar above the board and the transport a bar below
// it, and both are always on screen: who is playing and what the score is are read the whole way
// through a match, so they are not something to go and find. The tools -- zoom and the territory
// toggle -- are the only layer over the stage, and appear on hover, on keyboard focus and on a
// touch, the way a video player's chrome does. `chrome: "always"` pins them open.
//
// **The chrome follows the page; the board does not.** Every colour of the frame is one of the
// platform's design tokens with a written-out fallback, so the player is the colour of the card it
// sits in and follows a theme switch with no work here. The board keeps its own fixed palette in
// both themes: a match has to look like itself, the way a video does not change colour with the
// player around it.
//
// **Every rule in the stylesheet starts at `.tb-viz`.** One `<style>` goes into the host document
// on mount; it is not a shadow root, and `check.mjs` fails the build on an unscoped rule. An
// unscoped `.tb-bar` in here once landed on the web application's own header.

import { allFrames } from "./engine.js";
import { Renderer, SEATS, expandRle } from "./render.js";

const CSS = `
.tb-viz{
  /* Roles, not literals: var(--ink, …) is the platform's token where the host loads
     design-system/tokens.css, and the written-out Cobalt value everywhere else. */
  --tb-ink:var(--ink,#142642);
  --tb-dim:var(--muted,#536780);
  --tb-line:var(--line,#B9C9E1);
  --tb-panel:var(--surface,#FFFFFF);
  --tb-raised:var(--surface-raised,#EAF0FC);
  --tb-accent:var(--accent,#255FC5);
  --tb-accent-ink:var(--accent-ink,#FFFFFF);
  --tb-bad:var(--danger,#BF354D);
  --tb-sans:var(--font-sans,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif);
  --tb-mono:var(--font-mono,ui-monospace,"SF Mono",Menlo,Consolas,monospace);
  --tb-r:var(--radius-sm,6px);
  /* The board's own ground, and the tray that is read against it. Fixed in both themes. */
  --tb-void:#05080F;
  --tb-over:#0D1729;
  --tb-over-line:rgba(238,243,255,.16);
  --tb-over-ink:#EEF3FF;
  --tb-over-dim:#A6B5D1;

  color:var(--tb-ink);background:var(--tb-panel);
  display:flex;flex-direction:column;min-height:0;
  font:13px/1.5 var(--tb-sans);
  overflow:hidden;-webkit-font-smoothing:antialiased}
/* No border and no radius of its own. mount() styles the host element rather than making a root
   inside it, so a frame the host has already drawn -- the web application's card, the book's figure
   -- is the frame, and a second one drawn here would sit inside it. Clipping is this side's: the
   stage paints to the corners and something has to cut them. */

/* The host page's dark palette, for a host that ships no tokens of its own. Where it does ship
   them the var() above already answered and every line here resolves to the same value twice. */
@media (prefers-color-scheme:dark){.tb-viz:not([data-tb-theme=light]){
  --tb-ink:var(--ink,#EEF3FF);--tb-dim:var(--muted,#A6B5D1);--tb-line:var(--line,#344764);
  --tb-panel:var(--surface,#121D32);--tb-raised:var(--surface-raised,#1C2B46);
  --tb-accent:var(--accent,#86B2FF);--tb-accent-ink:var(--accent-ink,#0B224A);
  --tb-bad:var(--danger,#FF9A9A)}}

/* An asked-for theme is authoritative: literals, so theme:"dark" is dark inside a light page. */
.tb-viz[data-tb-theme=light]{
  --tb-ink:#142642;--tb-dim:#536780;--tb-line:#B9C9E1;--tb-panel:#FFFFFF;--tb-raised:#EAF0FC;
  --tb-accent:#255FC5;--tb-accent-ink:#FFFFFF;--tb-bad:#BF354D}
.tb-viz[data-tb-theme=dark]{
  --tb-ink:#EEF3FF;--tb-dim:#A6B5D1;--tb-line:#344764;--tb-panel:#121D32;--tb-raised:#1C2B46;
  --tb-accent:#86B2FF;--tb-accent-ink:#0B224A;--tb-bad:#FF9A9A}

.tb-viz *{box-sizing:border-box}
/* The root sets display:flex on the host element, which outranks the [hidden] attribute's UA rule
   unless it is said again here -- and a frame that hides the viewer while it loads has to work. */
.tb-viz[hidden]{display:none}
.tb-viz:focus{outline:none}
.tb-viz:focus-visible{outline:2px solid var(--tb-accent);outline-offset:-2px}

/* ---------- the title bar: every seat, always on screen ----------
   Colour, name and score first, and never squeezed out; whose it is and the counts after them, and
   those are what give way when the frame is narrow. One line at any width, so a count that grows by
   a digit never moves the board under it. */
.tb-viz .tb-top{display:flex;flex:none;min-width:0;background:var(--tb-panel);
  border-bottom:1px solid var(--tb-line)}
.tb-viz .tb-seat{display:flex;align-items:center;gap:8px;flex:1 1 0;min-width:0;padding:8px 12px;
  white-space:nowrap;overflow:hidden}
.tb-viz .tb-seat+.tb-seat{border-left:1px solid var(--tb-line)}
.tb-viz .tb-seat[data-out=true]{opacity:.55}
.tb-viz .tb-chip{width:10px;height:10px;border-radius:3px;flex:none}
/* The name does not shrink at all: flex shares a squeeze out in proportion, so a name allowed to
   shrink lost a few pixels of itself while the counts beside it kept a hundred. Only a name too long
   for its seat is cut, and never so far that the chip and the score lose their room beside it. */
.tb-viz .tb-name{font-weight:650;overflow:hidden;text-overflow:ellipsis;flex:none;
  max-width:min(20ch,calc(100% - 60px))}
.tb-viz .tb-score{font:600 15px/1 var(--tb-mono);font-variant-numeric:tabular-nums;flex:none}
.tb-viz .tb-by{color:var(--tb-dim);overflow:hidden;text-overflow:ellipsis;flex:0 100 auto;
  min-width:0}
.tb-viz .tb-nums{margin-left:auto;font:11px/1 var(--tb-mono);font-variant-numeric:tabular-nums;
  color:var(--tb-dim);overflow:hidden;text-overflow:ellipsis;flex:0 10 auto;min-width:0}

/* ---------- the stage ---------- */
.tb-viz .tb-stage{position:relative;flex:1 1 auto;min-height:200px;display:flex;overflow:hidden;
  background:var(--tb-void);cursor:grab}
.tb-viz .tb-stage.tb-drag{cursor:grabbing}
.tb-viz .tb-stage canvas{display:block;margin:auto}

/* ---------- the tray ----------
   One layer over the board, out of the way until it is wanted. Flat and opaque rather than a
   blurred pane: the pane read as terrain over a busy board, which is the one place it must not. */
.tb-viz .tb-tray{position:absolute;inset:0;display:flex;flex-direction:column;
  justify-content:space-between;gap:8px;padding:9px;pointer-events:none;
  opacity:0;transition:opacity .16s ease}
.tb-viz .tb-row{display:flex;align-items:flex-start;gap:8px;min-width:0}
.tb-viz .tb-head{transform:translateY(-5px);transition:transform .16s ease}
.tb-viz:is(:hover,:focus-within,[data-tb-peek],[data-tb-chrome=always]) .tb-tray{opacity:1}
.tb-viz:is(:hover,:focus-within,[data-tb-peek],[data-tb-chrome=always]) .tb-head{transform:none}
.tb-viz .tb-pane{background:var(--tb-over);border:1px solid var(--tb-over-line);
  border-radius:var(--tb-r);color:var(--tb-over-ink)}

/* The only part of the tray that is clickable, so a drag anywhere else still pans the board. */
.tb-viz .tb-tools{display:flex;gap:2px;margin-left:auto;padding:3px;pointer-events:auto;flex:none}
.tb-viz .tb-tools button{width:28px;height:28px;padding:0;border-radius:5px;
  color:var(--tb-over-ink)}
.tb-viz .tb-tools button:hover:not(:disabled){background:rgba(238,243,255,.14)}
.tb-viz .tb-tools button[aria-pressed=true]{background:rgba(238,243,255,.24)}
.tb-viz .tb-tools .tb-sep{width:1px;margin:5px 3px;background:var(--tb-over-line)}

/* ---------- the transport, which is always on screen ---------- */
.tb-viz .tb-bar{display:flex;align-items:center;gap:10px;padding:8px 10px;flex:none;
  background:var(--tb-panel);border-top:1px solid var(--tb-line)}
.tb-viz button{font:inherit;color:var(--tb-ink);background:transparent;border:1px solid transparent;
  border-radius:5px;padding:5px 7px;cursor:pointer;line-height:1;display:inline-flex;
  align-items:center;justify-content:center}
.tb-viz button:hover:not(:disabled){background:var(--tb-raised)}
.tb-viz button:disabled{opacity:.35;cursor:default}
.tb-viz button:focus-visible{outline:2px solid var(--tb-accent);outline-offset:2px}
.tb-viz .tb-transport{display:flex;gap:2px;align-items:center}
.tb-viz .tb-transport button{width:32px;height:32px;padding:0}
.tb-viz .tb-play{background:var(--tb-accent);color:var(--tb-accent-ink);
  width:36px;height:36px;border-radius:50%}
.tb-viz .tb-play:hover:not(:disabled){background:var(--tb-accent);filter:brightness(1.06)}

.tb-viz .tb-track{position:relative;flex:1 1 auto;height:28px;display:flex;align-items:center;
  cursor:pointer;touch-action:none;min-width:80px}
.tb-viz .tb-rail{position:absolute;left:0;right:0;height:5px;border-radius:3px;
  background:var(--tb-raised)}
.tb-viz .tb-fill{position:absolute;left:0;height:5px;border-radius:3px;background:var(--tb-accent)}
.tb-viz .tb-mark{position:absolute;width:2px;height:12px;border-radius:1px;
  transform:translateX(-1px);opacity:.9}
.tb-viz .tb-thumb{position:absolute;width:13px;height:13px;border-radius:50%;
  background:var(--tb-accent);border:2px solid var(--tb-panel);transform:translateX(-6.5px);
  box-shadow:0 1px 3px rgba(0,0,0,.3)}
.tb-viz .tb-turn{font:12px/1 var(--tb-mono);font-variant-numeric:tabular-nums;color:var(--tb-dim);
  white-space:nowrap;min-width:74px;text-align:right}
.tb-viz .tb-speed{font:11px/1 var(--tb-mono);min-width:36px;color:var(--tb-dim)}
.tb-viz .tb-err{padding:16px;color:var(--tb-bad);font:13px/1.6 var(--tb-sans)}

@media (prefers-reduced-motion:reduce){
  .tb-viz .tb-tray,.tb-viz .tb-head{transition:none}}
`;

const STYLE_ID = "tb-viz-style";

/**
 * One stylesheet per document, not one per process.
 *
 * A module-level "done" flag is the obvious shape and the wrong one: a second document — an
 * iframe, a print window, a test harness — would then get a viewer with no stylesheet at all,
 * because the first document had already claimed the flag.
 */
function injectCss(doc) {
  if (doc.getElementById(STYLE_ID)) return;
  const el = doc.createElement("style");
  el.id = STYLE_ID;
  el.textContent = CSS;
  doc.head.appendChild(el);
}

const SPEEDS = [0.25, 0.5, 1, 2, 4, 8];
const TURNS_PER_SECOND = 10;
/** How long the tray stays up after a touch, which has no hover to keep it up. */
const PEEK_MS = 2600;
/** How near an enemy ant has to be to a hill, in moves, before the hill is ringed. */
const THREAT_STEPS = 8;

// Inline SVG rather than glyphs: "⏮" renders as a different width, weight and baseline on every
// platform, and transport controls that jump about are the first thing that makes a player feel
// unfinished.
const ICON = {
  play: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z"/></svg>',
  pause: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="2.5" width="3" height="11" rx="1"/><rect x="9" y="2.5" width="3" height="11" rx="1"/></svg>',
  // One arrow steps; two arrows against a bar go to the end. At fifteen pixels a triangle with a
  // bar beside it and a bare triangle are the same button drawn twice.
  prev: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M11.4 3.2v9.6L4.6 8z"/></svg>',
  next: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M4.6 3.2v9.6L11.4 8z"/></svg>',
  first: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M13 3.6v8.8L9 8z"/><path d="M8.8 3.6v8.8L4.8 8z"/><rect x="2.7" y="3.2" width="1.7" height="9.6" rx=".85"/></svg>',
  last: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M3 3.6v8.8L7 8z"/><path d="M7.2 3.6v8.8L11.2 8z"/><rect x="11.6" y="3.2" width="1.7" height="9.6" rx=".85"/></svg>',
  plus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="7.2" y="3" width="1.6" height="10" rx=".8"/><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  minus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  fit: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M2.8 6V2.8H6M10 2.8h3.2V6M13.2 10v3.2H10M6 13.2H2.8V10"/></svg>',
  // An eye, because a seat's territory is what it has seen.
  explored: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M1.6 8s2.4-4.3 6.4-4.3S14.4 8 14.4 8s-2.4 4.3-6.4 4.3S1.6 8 1.6 8z"/><circle cx="8" cy="8" r="2" fill="currentColor" stroke="none"/></svg>',
};

export class Viewer {
  /**
   * @param {HTMLElement} el   where to draw
   * @param {object} replay    the envelope: it carries its own board, so nothing else is needed
   * @param {object} [opts]    { turn, from, to, autoplay, speed, theme, chrome, height, labels,
   *                             explored, onTurn }
   */
  constructor(el, replay, opts = {}) {
    this.el = el;
    this.replay = replay;
    this.opts = opts;
    this.onTurn = opts.onTurn;
    this.playing = false;
    this.speed = opts.speed ?? 1;
    this.raf = null;
    this.peekTimer = null;
    this.destroyed = false;

    injectCss(el.ownerDocument);
    el.innerHTML = "";
    el.classList.add("tb-viz");
    if (opts.theme) el.dataset.tbTheme = opts.theme;
    if (opts.chrome) el.dataset.tbChrome = opts.chrome;
    // A host that says how tall the player is says it once, here, rather than having to know that
    // the root is a flex column that will otherwise collapse to its bar.
    if (opts.height) el.style.height = typeof opts.height === "number" ? `${opts.height}px` : opts.height;

    try {
      // One pass over the match, on construction. Everything after this is an array lookup, which
      // is what makes scrubbing feel like scrubbing rather than like waiting.
      this.frames = allFrames(replay);
    } catch (e) {
      el.innerHTML = `<div class="tb-err">This replay could not be decoded.<br>${esc(e.message)}</div>`;
      return;
    }

    // A range narrows what the timeline covers without changing what a turn number means, so a
    // tutorial can point at turns 40-60 of a real match and the reader still sees "turn 47".
    this.lo = clamp(opts.from ?? 0, 0, this.frames.length - 1);
    this.hi = clamp(opts.to ?? this.frames.length - 1, this.lo, this.frames.length - 1);
    this.i = clamp(opts.turn ?? this.lo, this.lo, this.hi);

    this.labels = seatLabels(replay, this.frames[0].score.length, opts.labels);
    this.names = this.labels.map((l) => l.name);
    this.events = findEvents(this.frames, this.lo, this.hi);
    // Derived from the frames on first use and kept: the board's step counts on the first paint,
    // the territory the first time someone asks to see it.
    this.board = null;
    this.firstSeen = null;
    // Only a cartridge that reports what each seat saw can draw it. This bundle ships with one that
    // does; a shell copied beside an older engine would otherwise draw the whole board as fog.
    this.canExplore = Array.isArray(this.frames[0].discovered);
    this.explored = Boolean(opts.explored) && this.canExplore;

    this.build();
    this.renderer.setBoard(replay.map);
    this.observe();
    // A tutorial points at a turn AND a place: `zoom` is a multiple of the fitted scale, `centre`
    // is the cell to put in the middle. Applied after the first layout, because both are relative
    // to a viewport that does not exist until then.
    if (opts.zoom && opts.zoom > 1) {
      this.renderer.zoomAt(opts.zoom, 0, 0);
      const c = opts.centre ?? this.busiestCell();
      this.renderer.centreOn(c[0], c[1]);
    } else if (opts.centre) {
      this.renderer.centreOn(opts.centre[0], opts.centre[1]);
    }
    this.show();
    if (opts.autoplay) this.play();
  }

  // ------------------------------------------------------------------ building

  build() {
    const d = this.el.ownerDocument;
    const mk = (tag, cls, parent, html) => {
      const n = d.createElement(tag);
      if (cls) n.className = cls;
      if (html != null) n.innerHTML = html;
      (parent || this.el).appendChild(n);
      return n;
    };
    const btn = (parent, icon, title, fn, cls) => {
      const b = mk("button", cls, parent, icon);
      b.type = "button";
      b.title = title;
      b.setAttribute("aria-label", title);
      b.onclick = fn;
      return b;
    };

    // ---- the title bar: every seat, always on screen
    this.seats = mk("div", "tb-top");

    // ---- stage
    this.stage = mk("div", "tb-stage");
    this.canvas = mk("canvas", null, this.stage);
    this.canvas.setAttribute("role", "img");
    this.renderer = new Renderer(this.canvas);

    // ---- the tray: the tools, over the board's top-right corner
    const tray = mk("div", "tb-tray", this.stage);
    const head = mk("div", "tb-row tb-head", tray);
    const tools = mk("div", "tb-tools tb-pane", head);
    this.exploreBtn = btn(tools, ICON.explored, "Explored territory (E)", () =>
      this.setExplored(!this.explored)
    );
    this.exploreBtn.setAttribute("aria-pressed", String(this.explored));
    if (!this.canExplore) {
      this.exploreBtn.disabled = true;
      this.exploreBtn.title = "This engine does not report what each seat has seen";
    }
    mk("span", "tb-sep", tools);
    btn(tools, ICON.minus, "Zoom out (−)", () => this.zoom(1 / 1.4));
    btn(tools, ICON.plus, "Zoom in (+)", () => this.zoom(1.4));
    btn(tools, ICON.fit, "Fit the board (0)", () => {
      this.renderer.fit();
      this.paint();
    });

    // ---- transport
    const bar = mk("div", "tb-bar");
    const t = mk("div", "tb-transport", bar);
    this.firstBtn = btn(t, ICON.first, "First turn (Home)", () => this.seek(this.lo));
    this.prevBtn = btn(t, ICON.prev, "Previous turn (←)", () => this.step(-1));
    this.playBtn = btn(t, ICON.play, "Play (space)", () => (this.playing ? this.pause() : this.play()), "tb-play");
    this.nextBtn = btn(t, ICON.next, "Next turn (→)", () => this.step(1));
    this.lastBtn = btn(t, ICON.last, "Last turn (End)", () => this.seek(this.hi));

    this.buildTrack(mk("div", "tb-track", bar));
    this.turnLabel = mk("span", "tb-turn", bar);

    this.speedBtn = mk("button", "tb-speed", bar);
    this.speedBtn.type = "button";
    this.speedBtn.title = "Playback speed";
    this.speedBtn.onclick = () => {
      this.speed = SPEEDS[(SPEEDS.indexOf(this.speed) + 1) % SPEEDS.length];
      this.speedBtn.textContent = `${this.speed}×`;
    };
    this.speedBtn.textContent = `${this.speed}×`;

    this.buildSeats();

    // ---- input
    this.el.tabIndex = 0;
    this.el.addEventListener("keydown", (e) => this.key(e));
    this.stage.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
    this.stage.addEventListener("pointerdown", (e) => this.onDown(e));
    this.stage.addEventListener("pointermove", (e) => this.onMove(e));
    this.stage.addEventListener("pointerup", (e) => this.onUp(e));
  }

  /**
   * The title bar's seats, built once and then written to.
   *
   * They used to be rebuilt from scratch on every frame, which is ten times a second at 1× and
   * eighty at 8×: a row of elements thrown away and remade while the reader is trying to read the
   * numbers on it. Nothing about a seat changes but its score and its counts.
   */
  buildSeats() {
    const d = this.el.ownerDocument;
    const span = (row, cls, text) => {
      const n = d.createElement("span");
      n.className = cls;
      if (text != null) {
        n.textContent = text;
        n.title = text;
      }
      row.appendChild(n);
      return n;
    };
    this.seatRows = this.frames[this.i].score.map((_, seat) => {
      const row = d.createElement("div");
      row.className = "tb-seat";
      span(row, "tb-chip").style.background = SEATS[seat % SEATS.length];
      const { name, by } = this.labels[seat];
      span(row, "tb-name", name);
      const score = span(row, "tb-score");
      if (by) span(row, "tb-by", by);
      const nums = span(row, "tb-nums");
      this.seats.appendChild(row);
      return { row, score, nums };
    });
  }

  buildTrack(track) {
    const d = this.el.ownerDocument;
    this.track = track;
    track.setAttribute("role", "slider");
    track.setAttribute("aria-label", "Turn");
    const rail = d.createElement("div");
    rail.className = "tb-rail";
    track.appendChild(rail);
    this.fill = d.createElement("div");
    this.fill.className = "tb-fill";
    track.appendChild(this.fill);

    // Event marks: where a hill fell and where a colony ended. A two-hundred-turn match has three
    // or four moments in it, and without these they can only be found by scrubbing for them.
    for (const ev of this.events) {
      const m = d.createElement("div");
      m.className = "tb-mark";
      m.style.left = `${this.pct(ev.i)}%`;
      m.style.background = SEATS[ev.seat % SEATS.length];
      m.title = `turn ${this.frames[ev.i].turn}: ${ev.what}`;
      track.appendChild(m);
    }

    this.thumb = d.createElement("div");
    this.thumb.className = "tb-thumb";
    track.appendChild(this.thumb);

    // Click anywhere on the track to go there; drag to scrub.
    const at = (e) => {
      const r = track.getBoundingClientRect();
      const f = clamp((e.clientX - r.left) / Math.max(1, r.width), 0, 1);
      return Math.round(this.lo + f * (this.hi - this.lo));
    };
    track.addEventListener("pointerdown", (e) => {
      track.setPointerCapture(e.pointerId);
      this.scrubbing = true;
      this.pause();
      this.seek(at(e));
    });
    track.addEventListener("pointermove", (e) => {
      if (this.scrubbing) this.seek(at(e));
    });
    const end = (e) => {
      if (!this.scrubbing) return;
      this.scrubbing = false;
      try {
        track.releasePointerCapture(e.pointerId);
      } catch {}
    };
    track.addEventListener("pointerup", end);
    track.addEventListener("pointercancel", end);
  }

  observe() {
    const fit = () => {
      const r = this.stage.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) return;
      this.renderer.resize(r.width, r.height);
      this.paint();
    };
    this.ro = new ResizeObserver(fit);
    this.ro.observe(this.stage);
    fit();
  }

  /**
   * Where the action is: the densest cluster of ants, not their centre of mass.
   *
   * The mean is the wrong answer and wrong in the way that matters -- two colonies on opposite
   * sides of a board average to the empty middle, so a zoom that used it would open on nothing at
   * all. This buckets the board and takes the fullest bucket.
   */
  busiestCell() {
    const f = this.frames[this.i];
    const all = f.ants.length ? f.ants : f.hills;
    if (!all.length) return [this.renderer.rows / 2, this.renderer.cols / 2];
    const step = 8;
    const buckets = new Map();
    for (const a of all) {
      const k = `${(a[0] / step) | 0},${(a[1] / step) | 0}`;
      const b = buckets.get(k) ?? { n: 0, r: 0, c: 0 };
      b.n++;
      b.r += a[0];
      b.c += a[1];
      buckets.set(k, b);
    }
    let best = null;
    for (const b of buckets.values()) if (!best || b.n > best.n) best = b;
    return [best.r / best.n, best.c / best.n];
  }

  // ------------------------------------------------------------------ input

  key(e) {
    const k = e.key;
    const jump = e.shiftKey ? 10 : 1;
    const map = {
      " ": () => (this.playing ? this.pause() : this.play()),
      ArrowRight: () => this.step(jump),
      ArrowLeft: () => this.step(-jump),
      ArrowUp: () => this.step(10),
      ArrowDown: () => this.step(-10),
      Home: () => this.seek(this.lo),
      End: () => this.seek(this.hi),
      "+": () => this.zoom(1.4),
      "=": () => this.zoom(1.4),
      "-": () => this.zoom(1 / 1.4),
      0: () => {
        this.renderer.fit();
        this.paint();
      },
      e: () => this.setExplored(!this.explored),
      E: () => this.setExplored(!this.explored),
    };
    const fn = map[k];
    if (!fn) return;
    e.preventDefault();
    // The territory is worth watching grow, so turning it on is not a reason to stop.
    if (k !== " " && k !== "e" && k !== "E") this.pause();
    fn();
  }

  /**
   * Show the tray for a moment.
   *
   * Hover is what raises it, and a touch screen has no hover: without this the zoom buttons and the
   * territory toggle would be unreachable on a phone rather than merely out of the way.
   */
  peek() {
    if (this.destroyed) return;
    this.el.dataset.tbPeek = "1";
    if (this.peekTimer) clearTimeout(this.peekTimer);
    this.peekTimer = setTimeout(() => {
      delete this.el.dataset.tbPeek;
      this.peekTimer = null;
    }, PEEK_MS);
  }

  onWheel(e) {
    e.preventDefault();
    const r = this.canvas.getBoundingClientRect();
    this.renderer.zoomAt(e.deltaY < 0 ? 1.12 : 1 / 1.12, e.clientX - r.left, e.clientY - r.top);
    this.paint();
  }

  onDown(e) {
    if (e.pointerType === "touch") this.peek();
    if (e.target !== this.canvas && e.target !== this.stage) return;
    this.stage.setPointerCapture(e.pointerId);
    this.drag = { x: e.clientX, y: e.clientY };
    this.stage.classList.add("tb-drag");
  }

  onMove(e) {
    if (!this.drag) return;
    const dx = e.clientX - this.drag.x;
    const dy = e.clientY - this.drag.y;
    this.drag.x = e.clientX;
    this.drag.y = e.clientY;
    this.renderer.pan(dx, dy);
    this.paint();
  }

  onUp(e) {
    this.drag = null;
    this.stage.classList.remove("tb-drag");
    try {
      this.stage.releasePointerCapture(e.pointerId);
    } catch {}
  }

  zoom(f) {
    const { w, h } = this.renderer.viewport();
    this.renderer.zoomAt(f, w / 2, h / 2);
    this.paint();
  }

  // ------------------------------------------------------------------ playback

  step(n) {
    this.seek(this.i + n);
  }

  seek(turn) {
    const next = clamp(turn, this.lo, this.hi);
    if (next === this.i) return;
    this.i = next;
    this.show();
  }

  pct(i) {
    const span = Math.max(1, this.hi - this.lo);
    return ((i - this.lo) / span) * 100;
  }

  paint() {
    this.renderer.render(this.frames[this.i], { threats: this.threatsAt(this.i) });
  }

  show() {
    const f = this.frames[this.i];
    if (this.explored) this.renderer.setTerritory(this.territoryAt(this.i).mask);
    this.paint();

    const p = this.pct(this.i);
    this.fill.style.width = `${p}%`;
    this.thumb.style.left = `${p}%`;
    this.track.setAttribute("aria-valuenow", String(f.turn));
    this.track.setAttribute("aria-valuemin", String(this.frames[this.lo].turn));
    this.track.setAttribute("aria-valuemax", String(this.frames[this.hi].turn));
    this.turnLabel.textContent = `${f.turn} / ${this.frames[this.hi].turn}`;

    this.prevBtn.disabled = this.i <= this.lo;
    this.firstBtn.disabled = this.i <= this.lo;
    this.nextBtn.disabled = this.i >= this.hi;
    this.lastBtn.disabled = this.i >= this.hi;

    this.writeSeats();

    if (this.onTurn) this.onTurn(f);
  }

  /**
   * The title bar's scores and counts, and the canvas's description, which read the same counts.
   *
   * Apart from `show()` because the territory toggle changes what a chip says without changing the
   * turn, and `show()` also tells the host that the turn changed.
   */
  writeSeats() {
    const f = this.frames[this.i];
    // One pass for the counts the seats and the label both want, rather than one pass per seat.
    const ants = new Array(f.score.length).fill(0);
    for (const a of f.ants) ants[a[2]] = (ants[a[2]] ?? 0) + 1;
    const hills = new Array(f.score.length).fill(0);
    for (const h of f.hills) hills[h[2]] = (hills[h[2]] ?? 0) + 1;
    const seen = this.explored ? this.territoryAt(this.i).seen : null;
    const cells = this.renderer.rows * this.renderer.cols;
    const threats = this.threatsAt(this.i).map(
      (t) => `${this.names[t.owner]}'s hill has an enemy ${t.steps} move${t.steps === 1 ? "" : "s"} from it`
    );

    this.canvas.setAttribute(
      "aria-label",
      `Turn ${f.turn}. ${f.score
        .map((s, i) => `${this.names[i]}: ${ants[i]} ant${ants[i] === 1 ? "" : "s"}, score ${s}`)
        .concat(threats)
        .join(". ")}`
    );

    f.score.forEach((score, seat) => {
      const row = this.seatRows[seat];
      if (!row) return;
      row.row.dataset.out = String(ants[seat] === 0);
      row.score.textContent = String(score);
      row.nums.textContent =
        `${ants[seat]} ant${ants[seat] === 1 ? "" : "s"} · ${hills[seat]} hill${hills[seat] === 1 ? "" : "s"}` +
        (seen ? ` · ${Math.round((100 * seen[seat]) / cells)}% explored` : "");
    });
  }

  /** Show or hide each seat's explored territory. */
  setExplored(on) {
    this.explored = Boolean(on) && this.canExplore;
    this.exploreBtn.setAttribute("aria-pressed", String(this.explored));
    this.renderer.setTerritory(this.explored ? this.territoryAt(this.i).mask : null);
    this.paint();
    this.writeSeats();
  }

  /**
   * The hills an enemy ant is within `THREAT_STEPS` moves of at frame `i`, and how near the nearest
   * one is. Kept for the one frame, because `paint()` runs on every pan and zoom as well as every
   * turn.
   */
  threatsAt(i) {
    if (this.threatCache?.i === i) return this.threatCache.threats;
    const map = this.replay.map;
    if (!this.board && map) {
      const water = expandRle(map.water, map.rows * map.cols);
      this.board = { rows: map.rows, cols: map.cols, water, steps: new Map() };
    }
    const threats = this.board ? threatsIn(this.frames[i], this.board, THREAT_STEPS) : [];
    this.threatCache = { i, threats };
    return threats;
  }

  /**
   * What each seat has explored at frame `i`: a byte per cell with bit `s` set where seat `s` knows
   * it, and how many cells each seat knows.
   *
   * The frames' `discovered` -- the engine's own record of what each seat saw first, turn by turn --
   * is folded once into the frame at which each seat first saw each cell. After that any frame's
   * territory is one comparison per cell, which is what lets it keep up with playback at 8×.
   */
  territoryAt(i) {
    if (this.territoryCache?.i === i) return this.territoryCache;
    const cols = this.renderer.cols;
    const cells = this.renderer.rows * cols;
    this.firstSeen ??= firstSeen(this.frames, cols, cells);
    const mask = new Uint8Array(cells);
    const seen = this.firstSeen.map((at, seat) => {
      let n = 0;
      for (let k = 0; k < cells; k++) {
        if (at[k] <= i) {
          mask[k] |= 1 << seat;
          n++;
        }
      }
      return n;
    });
    this.territoryCache = { i, mask, seen };
    return this.territoryCache;
  }

  play() {
    if (this.playing || this.destroyed) return;
    if (this.i >= this.hi) this.seek(this.lo);
    this.playing = true;
    this.playBtn.innerHTML = ICON.pause;
    this.playBtn.title = "Pause (space)";
    this.playBtn.setAttribute("aria-label", "Pause (space)");
    let last = performance.now();
    let acc = 0;
    const tick = (now) => {
      if (!this.playing) return;
      acc += (now - last) * this.speed;
      last = now;
      const per = 1000 / TURNS_PER_SECOND;
      let moved = false;
      while (acc >= per) {
        acc -= per;
        if (this.i >= this.hi) {
          this.pause();
          return;
        }
        this.i++;
        moved = true;
      }
      if (moved) this.show();
      this.raf = requestAnimationFrame(tick);
    };
    this.raf = requestAnimationFrame(tick);
  }

  pause() {
    if (!this.playing) return;
    this.playing = false;
    this.playBtn.innerHTML = ICON.play;
    this.playBtn.title = "Play (space)";
    this.playBtn.setAttribute("aria-label", "Play (space)");
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = null;
  }

  destroy() {
    this.destroyed = true;
    this.pause();
    if (this.peekTimer) clearTimeout(this.peekTimer);
    if (this.ro) this.ro.disconnect();
    this.el.innerHTML = "";
    this.el.classList.remove("tb-viz");
    delete this.el.dataset.tbPeek;
  }
}

// ---------------------------------------------------------------- helpers

/**
 * The turns worth jumping to.
 *
 * A hill falls and a colony ends: both are visible in the frames without knowing any rule, because
 * a hill leaving the list means it was razed and an ant count reaching zero means a seat is out.
 */
function findEvents(frames, lo, hi) {
  const out = [];
  for (let i = lo + 1; i <= hi; i++) {
    const a = frames[i - 1];
    const b = frames[i];
    for (let seat = 0; seat < b.score.length; seat++) {
      const wasHills = a.hills.filter((h) => h[2] === seat).length;
      const nowHills = b.hills.filter((h) => h[2] === seat).length;
      if (nowHills < wasHills) out.push({ i, seat, what: `hill razed` });
      if (count(a.ants, seat) > 0 && count(b.ants, seat) === 0) {
        out.push({ i, seat, what: `colony wiped out` });
      }
    }
  }
  return out;
}

function count(ants, seat) {
  let n = 0;
  for (const a of ants) if (a[2] === seat) n++;
  return n;
}

/**
 * What to call each seat: `{ name, by }`. What the host says, else what the replay says, else the
 * hash, else the seat number.
 *
 * The envelope names a seat as the referee knew it -- by the hash of its weights, or by a label a
 * local run chose -- and a host usually knows better: the web application knows the model's name
 * and whose it is, and showed eight characters of a hash here while every other panel on the same
 * page said "mover by @someone". `by` is the host's own words, shown as given; who owns a model is
 * the platform's business, not the cartridge's.
 */
export function seatLabels(replay, n, given) {
  const out = [];
  for (let i = 0; i < n; i++) out[i] = { name: `seat ${i}`, by: "" };
  if (Array.isArray(replay.seats)) {
    for (const s of replay.seats) {
      const i = s.seat ?? 0;
      if (!out[i]) continue;
      out[i].name = s.label ?? (s.weights_hash ? s.weights_hash.slice(7, 15) : out[i].name);
    }
  }
  if (Array.isArray(given)) {
    for (const g of given) {
      const seat = out[g?.seat];
      if (!seat) continue;
      if (g.name) seat.name = String(g.name);
      if (g.by) seat.by = String(g.by);
    }
  }
  return out;
}

/**
 * How many moves from `from` each cell is, out to `reach`, on a board that wraps; -1 beyond it.
 *
 * Water is the only wall. Food blocks a move too, but it comes and goes, and a ring that flickered
 * as food fell would be drawing the food rather than the danger. This decides nothing about the
 * match: nothing reads it but the pen that rings a hill.
 */
export function stepsFrom(water, rows, cols, from, reach) {
  const dist = new Int8Array(rows * cols).fill(-1);
  dist[from] = 0;
  let edge = [from];
  for (let d = 1; d <= reach && edge.length; d++) {
    const next = [];
    for (const i of edge) {
      const r = (i / cols) | 0;
      const c = i - r * cols;
      const around = [
        ((r + rows - 1) % rows) * cols + c,
        ((r + 1) % rows) * cols + c,
        r * cols + ((c + cols - 1) % cols),
        r * cols + ((c + 1) % cols),
      ];
      for (const j of around) {
        if (dist[j] !== -1 || water[j]) continue;
        dist[j] = d;
        next.push(j);
      }
    }
    edge = next;
  }
  return dist;
}

/**
 * The hills with an enemy ant within `reach` moves, each with how many moves the nearest one is.
 *
 * Moves, not distance as the crow flies: on a maze an enemy one wall away from a hill can be thirty
 * moves from it, and a ring that lit up for it would point at nothing. Hills never move, so each
 * one's step counts are worked out once and kept on `board.steps`.
 */
export function threatsIn(frame, board, reach) {
  const out = [];
  for (const [r, c, owner] of frame.hills) {
    const at = r * board.cols + c;
    let steps = board.steps.get(at);
    if (!steps) {
      steps = stepsFrom(board.water, board.rows, board.cols, at, reach);
      board.steps.set(at, steps);
    }
    let best = -1;
    for (const [ar, ac, ao] of frame.ants) {
      if (ao === owner) continue;
      const d = steps[ar * board.cols + ac];
      if (d >= 0 && (best < 0 || d < best)) best = d;
    }
    if (best >= 0) out.push({ r, c, owner, steps: best, reach });
  }
  return out;
}

/**
 * For each seat, the frame at which it first saw each cell, or 65535 where it never did.
 *
 * The frames are decoded from turn zero, so the first one's `discovered` is the opening vision and
 * every cell a seat ever knows is announced exactly once after that.
 */
function firstSeen(frames, cols, cells) {
  const at = frames[0].score.map(() => new Uint16Array(cells).fill(0xffff));
  frames.forEach((f, i) => {
    (f.discovered ?? []).forEach((list, seat) => {
      const mine = at[seat];
      if (!mine) return;
      for (const [r, c] of list) {
        const k = r * cols + c;
        if (mine[k] === 0xffff) mine[k] = i;
      }
    });
  });
  return at;
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function esc(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}
