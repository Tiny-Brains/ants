// The player: everything around the pixels.
//
// It behaves like a media player because that is what watching a match is. Transport buttons, a
// timeline you can click and drag, playback speed, and a keyboard that does what a keyboard does
// in a player — space, arrows, home and end.
//
// Two things it does that a video player cannot, because a match is not video:
//
//   * **The timeline is annotated.** A hill razed and a colony wiped out are marked on the track,
//     so the interesting turns of a two-hundred-turn match can be found without scrubbing for them.
//   * **The board can be inspected.** Zoom in, and click a cell to see what is on it. A replay is
//     evidence about a model's decisions, and the question is usually "why did it do that, there".
//
// `docs/cartridge.md` §7 used to put this in the platform's web application and leave the cartridge
// owning pixels. It lives here because the viewer has three consumers that are not one application
// — the web Replay screen, the book's tutorials, and `tinybrains view` — and a shell split across
// three of them is a shell maintained in three places.
//
// Framework-free on purpose: this is a canvas, a slider and some readouts, and React would tie the
// cartridge to a version it has no business pinning. `react.js` wraps it for the application.

import { allFrames } from "./engine.js";
import { Renderer, SEATS } from "./render.js";

const CSS = `
.tb-viz{--tb-fg:#191c18;--tb-dim:#71776e;--tb-line:#dfe1d8;--tb-bg:#f4f5f0;--tb-panel:#fbfbf8;
  --tb-hover:#eceee5;--tb-accent:#2f7d8f;
  color:var(--tb-fg);background:var(--tb-bg);display:flex;flex-direction:column;min-height:0;
  font:13px/1.5 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;
  border:1px solid var(--tb-line);border-radius:6px;overflow:hidden;-webkit-font-smoothing:antialiased}
@media (prefers-color-scheme:dark){.tb-viz:not([data-tb-theme=light]){
  --tb-fg:#e9eae3;--tb-dim:#969c92;--tb-line:#2b2f2a;--tb-bg:#131512;--tb-panel:#191c18;
  --tb-hover:#232722;--tb-accent:#59b3c6}}
.tb-viz[data-tb-theme=dark]{--tb-fg:#e9eae3;--tb-dim:#969c92;--tb-line:#2b2f2a;--tb-bg:#131512;
  --tb-panel:#191c18;--tb-hover:#232722;--tb-accent:#59b3c6}
.tb-viz *{box-sizing:border-box}
.tb-viz:focus{outline:none}
.tb-viz:focus-visible{outline:2px solid var(--tb-accent);outline-offset:-2px}

.tb-stage{position:relative;flex:1 1 auto;min-height:220px;display:flex;overflow:hidden;
  background:#1b2229;cursor:grab}
.tb-stage.tb-drag{cursor:grabbing}
.tb-stage canvas{display:block;margin:auto}

.tb-badge{position:absolute;top:10px;left:10px;display:flex;gap:6px;align-items:center;
  padding:4px 9px;border-radius:99px;background:rgba(20,26,30,.72);color:#eef1ec;
  font:11px/1 ui-monospace,SFMono-Regular,Menlo,monospace;letter-spacing:.04em;
  backdrop-filter:blur(6px);pointer-events:none}
.tb-zoomers{position:absolute;top:10px;right:10px;display:flex;gap:4px}
.tb-zoomers button{width:28px;height:28px;padding:0;border-radius:6px;
  background:rgba(20,26,30,.72);color:#eef1ec;border:0;backdrop-filter:blur(6px)}
.tb-zoomers button:hover{background:rgba(20,26,30,.9)}
.tb-tip{position:absolute;bottom:10px;left:10px;padding:5px 9px;border-radius:5px;
  background:rgba(20,26,30,.82);color:#eef1ec;font:11px/1.45 ui-monospace,Menlo,monospace;
  backdrop-filter:blur(6px);pointer-events:none;white-space:pre}

.tb-bar{display:flex;align-items:center;gap:10px;padding:9px 11px;background:var(--tb-panel);
  border-top:1px solid var(--tb-line)}
.tb-viz button{font:inherit;color:var(--tb-fg);background:transparent;border:1px solid transparent;
  border-radius:5px;padding:5px 7px;cursor:pointer;line-height:1;display:inline-flex;
  align-items:center;justify-content:center}
.tb-viz button:hover:not(:disabled){background:var(--tb-hover)}
.tb-viz button:disabled{opacity:.35;cursor:default}
.tb-viz button:focus-visible{outline:2px solid var(--tb-accent);outline-offset:1px}
.tb-play{background:var(--tb-accent)!important;color:#fff!important;width:32px;height:32px;padding:0!important}
.tb-play:hover{filter:brightness(1.08)}
.tb-transport{display:flex;gap:2px;align-items:center}
.tb-speed{font:11px/1 ui-monospace,Menlo,monospace;min-width:34px;color:var(--tb-dim)}

.tb-track{position:relative;flex:1 1 auto;height:26px;display:flex;align-items:center;
  cursor:pointer;touch-action:none;min-width:80px}
.tb-rail{position:absolute;left:0;right:0;height:5px;border-radius:3px;background:var(--tb-line)}
.tb-fill{position:absolute;left:0;height:5px;border-radius:3px;background:var(--tb-accent)}
.tb-mark{position:absolute;width:2px;height:11px;border-radius:1px;transform:translateX(-1px);opacity:.85}
.tb-thumb{position:absolute;width:13px;height:13px;border-radius:50%;background:var(--tb-accent);
  border:2px solid var(--tb-panel);transform:translateX(-6.5px);box-shadow:0 1px 3px rgba(0,0,0,.28)}
.tb-turn{font:12px/1 ui-monospace,SFMono-Regular,Menlo,monospace;font-variant-numeric:tabular-nums;
  color:var(--tb-dim);white-space:nowrap;min-width:74px;text-align:right}

.tb-seats{display:flex;gap:8px;padding:9px 11px;background:var(--tb-panel);
  border-top:1px solid var(--tb-line);flex-wrap:wrap;align-items:stretch}
.tb-seat{display:flex;align-items:center;gap:8px;padding:5px 9px;border-radius:5px;
  border:1px solid var(--tb-line);min-width:0}
.tb-seat[data-out=true]{opacity:.45}
.tb-chip{width:9px;height:9px;border-radius:2px;flex:none}
.tb-name{font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:15ch}
.tb-nums{font:11px/1 ui-monospace,Menlo,monospace;font-variant-numeric:tabular-nums;color:var(--tb-dim);
  white-space:nowrap}
.tb-meta{margin-left:auto;align-self:center;font:11px/1.5 ui-monospace,Menlo,monospace;
  color:var(--tb-dim);text-align:right;white-space:nowrap}
.tb-err{padding:16px;color:#b0431f;font-size:13px}
`;

let cssDone = false;
function injectCss(doc) {
  if (cssDone) return;
  const el = doc.createElement("style");
  el.textContent = CSS;
  doc.head.appendChild(el);
  cssDone = true;
}

const SPEEDS = [0.25, 0.5, 1, 2, 4, 8];
const TURNS_PER_SECOND = 10;

// Inline SVG rather than glyphs: "⏮" renders as a different width, weight and baseline on every
// platform, and transport controls that jump about are the first thing that makes a player feel
// unfinished.
const ICON = {
  play: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z"/></svg>',
  pause: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="2.5" width="3" height="11" rx="1"/><rect x="9" y="2.5" width="3" height="11" rx="1"/></svg>',
  prev: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M10.5 3v10L5 8z"/><rect x="3.5" y="3" width="1.6" height="10" rx=".8"/></svg>',
  next: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M5.5 3v10L11 8z"/><rect x="10.9" y="3" width="1.6" height="10" rx=".8"/></svg>',
  first: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M12 3v10L6.5 8z"/><rect x="3.5" y="3" width="1.8" height="10" rx=".9"/></svg>',
  last: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M4 3v10L9.5 8z"/><rect x="10.7" y="3" width="1.8" height="10" rx=".9"/></svg>',
  plus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="7.2" y="3" width="1.6" height="10" rx=".8"/><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  minus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  fit: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M2.8 6V2.8H6M10 2.8h3.2V6M13.2 10v3.2H10M6 13.2H2.8V10"/></svg>',
};

export class Viewer {
  /**
   * @param {HTMLElement} el   where to draw
   * @param {object} replay    the envelope: it carries its own board, so nothing else is needed
   * @param {object} [opts]    { turn, from, to, autoplay, speed, theme, onTurn }
   */
  constructor(el, replay, opts = {}) {
    this.el = el;
    this.replay = replay;
    this.opts = opts;
    this.onTurn = opts.onTurn;
    this.playing = false;
    this.speed = opts.speed ?? 1;
    this.raf = null;
    this.destroyed = false;

    injectCss(el.ownerDocument);
    el.innerHTML = "";
    el.classList.add("tb-viz");
    if (opts.theme) el.dataset.tbTheme = opts.theme;

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

    this.names = seatNames(replay, this.frames[0].score.length);
    this.events = findEvents(this.frames, this.lo, this.hi);

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
      b.title = title;
      b.setAttribute("aria-label", title);
      b.onclick = fn;
      return b;
    };

    // ---- stage
    this.stage = mk("div", "tb-stage");
    this.canvas = mk("canvas", null, this.stage);
    this.canvas.setAttribute("role", "img");
    this.renderer = new Renderer(this.canvas);

    this.badge = mk("div", "tb-badge", this.stage);
    const zoomers = mk("div", "tb-zoomers", this.stage);
    btn(zoomers, ICON.minus, "Zoom out (−)", () => this.zoom(1 / 1.4));
    btn(zoomers, ICON.plus, "Zoom in (+)", () => this.zoom(1.4));
    btn(zoomers, ICON.fit, "Fit the board (0)", () => {
      this.renderer.fit();
      this.paint();
    });
    this.tip = mk("div", "tb-tip", this.stage);
    this.tip.style.display = "none";

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
    this.speedBtn.title = "Playback speed";
    this.speedBtn.onclick = () => {
      this.speed = SPEEDS[(SPEEDS.indexOf(this.speed) + 1) % SPEEDS.length];
      this.speedBtn.textContent = `${this.speed}×`;
    };
    this.speedBtn.textContent = `${this.speed}×`;

    // ---- seats
    this.seats = mk("div", "tb-seats");
    this.meta = mk("div", "tb-meta");

    // ---- input
    this.el.tabIndex = 0;
    this.el.addEventListener("keydown", (e) => this.key(e));
    this.stage.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
    this.stage.addEventListener("pointerdown", (e) => this.onDown(e));
    this.stage.addEventListener("pointermove", (e) => this.onMove(e));
    this.stage.addEventListener("pointerup", (e) => this.onUp(e));
    this.stage.addEventListener("pointerleave", () => {
      this.tip.style.display = "none";
      this.hover = null;
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
    };
    const fn = map[k];
    if (!fn) return;
    e.preventDefault();
    if (k !== " ") this.pause();
    fn();
  }

  onWheel(e) {
    e.preventDefault();
    const r = this.canvas.getBoundingClientRect();
    this.renderer.zoomAt(e.deltaY < 0 ? 1.12 : 1 / 1.12, e.clientX - r.left, e.clientY - r.top);
    this.paint();
  }

  onDown(e) {
    if (e.target !== this.canvas && e.target !== this.stage) return;
    this.stage.setPointerCapture(e.pointerId);
    this.drag = { x: e.clientX, y: e.clientY, moved: false };
    this.stage.classList.add("tb-drag");
  }

  onMove(e) {
    const r = this.canvas.getBoundingClientRect();
    if (this.drag) {
      const dx = e.clientX - this.drag.x;
      const dy = e.clientY - this.drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) this.drag.moved = true;
      this.drag.x = e.clientX;
      this.drag.y = e.clientY;
      this.renderer.pan(dx, dy);
      this.paint();
      return;
    }
    this.hover = this.renderer.cellAt(e.clientX - r.left, e.clientY - r.top);
    this.showTip();
  }

  onUp(e) {
    const wasDrag = this.drag && this.drag.moved;
    this.drag = null;
    this.stage.classList.remove("tb-drag");
    try {
      this.stage.releasePointerCapture(e.pointerId);
    } catch {}
    // A click that did not pan is an inspection: pin what is on that cell.
    if (!wasDrag) {
      const r = this.canvas.getBoundingClientRect();
      this.pinned = this.renderer.cellAt(e.clientX - r.left, e.clientY - r.top);
      this.hover = this.pinned;
      this.showTip();
    }
  }

  /** What is on the cell under the pointer, at this turn. */
  showTip() {
    const cell = this.hover;
    if (!cell) {
      this.tip.style.display = "none";
      return;
    }
    const [r, c] = cell;
    const f = this.frames[this.i];
    const lines = [`r${r} c${c}`];
    const ant = f.ants.find((a) => a[0] === r && a[1] === c);
    const hill = f.hills.find((h) => h[0] === r && h[1] === c);
    const food = f.food.some((x) => x[0] === r && x[1] === c);
    if (ant) lines.push(`ant · ${this.names[ant[2]]}`);
    if (hill) lines.push(`hill · ${this.names[hill[2]]}`);
    if (food) lines.push("food");
    if (!ant && !hill && !food) lines.push(isWater(this.replay.map, r, c) ? "water" : "land");
    this.tip.textContent = lines.join("\n");
    this.tip.style.display = "block";
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
    this.renderer.render(this.frames[this.i]);
    const s = this.renderer.scale;
    // rows x cols, which is how the game states a board everywhere else -- the map file, the
    // preset table, the book. A viewer that said 96x64 beside prose saying "64 by 96" would make
    // the reader stop and work out which of them was wrong.
    this.badge.textContent =
      `${this.replay.map_id ?? ""} ${this.renderer.rows}×${this.renderer.cols} rows×cols` +
      `  ${s.toFixed(1)}px/cell`;
  }

  show() {
    const f = this.frames[this.i];
    this.paint();
    this.showTip();

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

    this.canvas.setAttribute(
      "aria-label",
      `Turn ${f.turn}. ${f.score
        .map((s, i) => {
          const n = count(f.ants, i);
          return `${this.names[i]}: ${n} ant${n === 1 ? "" : "s"}, score ${s}`;
        })
        .join(". ")}`
    );

    this.seats.innerHTML = "";
    const d = this.el.ownerDocument;
    f.score.forEach((score, seat) => {
      const alive = count(f.ants, seat);
      const row = d.createElement("div");
      row.className = "tb-seat";
      row.dataset.out = String(alive === 0);
      const chip = d.createElement("span");
      chip.className = "tb-chip";
      chip.style.background = SEATS[seat % SEATS.length];
      const name = d.createElement("span");
      name.className = "tb-name";
      name.textContent = this.names[seat];
      const nums = d.createElement("span");
      nums.className = "tb-nums";
      const hills = f.hills.filter((h) => h[2] === seat).length;
      nums.textContent =
        `${score >= 0 ? "+" : ""}${score} · ${alive} ant${alive === 1 ? "" : "s"}` +
        ` · ${hills} hill${hills === 1 ? "" : "s"}`;
      row.append(chip, name, nums);
      this.seats.appendChild(row);
    });
    this.seats.appendChild(this.meta);
    const done = this.i === this.hi && this.replay.reason;
    this.meta.textContent = done
      ? `${this.replay.reason} after ${this.replay.turns} turns`
      : `seed ${this.replay.seed ?? "?"}`;

    if (this.onTurn) this.onTurn(f);
  }

  play() {
    if (this.playing || this.destroyed) return;
    if (this.i >= this.hi) this.seek(this.lo);
    this.playing = true;
    this.playBtn.innerHTML = ICON.pause;
    this.playBtn.title = "Pause (space)";
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
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = null;
  }

  destroy() {
    this.destroyed = true;
    this.pause();
    if (this.ro) this.ro.disconnect();
    this.el.innerHTML = "";
    this.el.classList.remove("tb-viz");
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

function isWater(map, r, c) {
  if (!map) return false;
  const target = r * map.cols + c;
  let i = 0;
  for (let k = 0; k + 1 < map.water.length; k += 2) {
    i += map.water[k + 1];
    if (target < i) return map.water[k] === 1;
  }
  return false;
}

/** Seat labels: what the replay says, else the hash, else the seat number. */
function seatNames(replay, n) {
  const out = [];
  for (let i = 0; i < n; i++) out[i] = `seat ${i}`;
  const seats = replay.seats;
  if (Array.isArray(seats)) {
    for (const s of seats) {
      const i = s.seat ?? 0;
      out[i] = s.label ?? (s.weights_hash ? s.weights_hash.slice(7, 15) : out[i]);
    }
  }
  return out;
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function esc(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}
