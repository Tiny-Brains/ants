// The shell: everything around the pixels.
//
// `docs/cartridge.md` §7 used to put this in the platform's web application and leave the cartridge
// "owning pixels". It lives here instead, because the viewer has three consumers that are not one
// application -- the web Replay screen, the book's tutorials, and `tinybrains view` -- and a shell
// split across three of them is a shell maintained in three places. The trade is written down in
// §7: the second cartridge writes its own scrubber, and whatever turns out to be genuinely
// game-independent moves to a shared package then, informed by two real viewers instead of one
// imagined one.
//
// It is framework-free on purpose. This is a canvas, a slider and a few readouts; React buys it
// nothing and would tie the cartridge to a React version. `react.js` wraps this for the web
// application, which is a real component to whoever consumes it.

import { allFrames } from "./engine.js";
import { Renderer, SEATS } from "./render.js";

const CSS = `
.tb-viz { --tb-fg:#1b1d1a; --tb-dim:#6e736c; --tb-line:#dcded5; --tb-bg:#f7f7f3; --tb-panel:#fff;
  color:var(--tb-fg); background:var(--tb-bg); font:14px/1.5 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,sans-serif;
  border:1px solid var(--tb-line); display:flex; flex-direction:column; gap:0; }
@media (prefers-color-scheme: dark) { .tb-viz:not([data-tb-theme=light]) {
  --tb-fg:#e8e9e3; --tb-dim:#9aa096; --tb-line:#2c302a; --tb-bg:#141613; --tb-panel:#1a1d19; } }
.tb-viz[data-tb-theme=dark] { --tb-fg:#e8e9e3; --tb-dim:#9aa096; --tb-line:#2c302a; --tb-bg:#141613; --tb-panel:#1a1d19; }
.tb-viz-board { display:flex; align-items:center; justify-content:center; padding:12px; min-height:0; }
.tb-viz-board canvas { image-rendering:pixelated; display:block; }
.tb-viz-bar { display:flex; align-items:center; gap:10px; padding:8px 12px; border-top:1px solid var(--tb-line); background:var(--tb-panel); flex-wrap:wrap; }
.tb-viz button { font:inherit; color:var(--tb-fg); background:transparent; border:1px solid var(--tb-line);
  border-radius:3px; padding:3px 9px; cursor:pointer; min-width:34px; }
.tb-viz button:hover { border-color:var(--tb-dim); }
.tb-viz button:focus-visible { outline:2px solid #2b7f95; outline-offset:2px; }
.tb-viz input[type=range] { flex:1 1 160px; min-width:120px; accent-color:#2b7f95; }
.tb-viz-turn { font-variant-numeric:tabular-nums; color:var(--tb-dim); white-space:nowrap; }
.tb-viz-seats { display:flex; gap:14px; padding:8px 12px; border-top:1px solid var(--tb-line);
  background:var(--tb-panel); flex-wrap:wrap; align-items:center; }
.tb-viz-seat { display:flex; align-items:center; gap:6px; white-space:nowrap; }
.tb-viz-chip { width:10px; height:10px; border-radius:2px; flex:none; }
.tb-viz-score { font-variant-numeric:tabular-nums; }
.tb-viz-meta { color:var(--tb-dim); font-size:12px; margin-left:auto; white-space:nowrap; }
.tb-viz-err { padding:14px; color:#b0431f; }
`;

let cssInjected = false;
function injectCss(doc) {
  if (cssInjected) return;
  const el = doc.createElement("style");
  el.textContent = CSS;
  doc.head.appendChild(el);
  cssInjected = true;
}

const SPEEDS = [0.5, 1, 2, 4, 8];

export class Viewer {
  /**
   * @param {HTMLElement} el      where to draw
   * @param {object} replay       the envelope: it carries its own board, so nothing else is needed
   * @param {object} [opts]       { turn, from, to, autoplay, speed, theme, onTurn }
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
      // is what makes scrubbing feel like scrubbing.
      this.frames = allFrames(replay);
    } catch (e) {
      el.innerHTML = `<div class="tb-viz-err">This replay could not be decoded.<br>${escape(e.message)}</div>`;
      return;
    }

    // A range narrows what the timeline covers without changing what a turn number means, so a
    // book page can point at turns 40-60 of a real match and the reader still sees "turn 47".
    this.lo = clamp(opts.from ?? 0, 0, this.frames.length - 1);
    this.hi = clamp(opts.to ?? this.frames.length - 1, this.lo, this.frames.length - 1);
    this.i = clamp(opts.turn ?? this.lo, this.lo, this.hi);

    this.build();
    this.renderer.setBoard(replay.map);
    this.fit();
    this.show();

    this.ro = new ResizeObserver(() => this.fit());
    this.ro.observe(this.board);
    if (opts.autoplay) this.play();
  }

  build() {
    const d = this.el.ownerDocument;
    const mk = (tag, cls, parent) => {
      const n = d.createElement(tag);
      if (cls) n.className = cls;
      (parent || this.el).appendChild(n);
      return n;
    };

    this.board = mk("div", "tb-viz-board");
    this.canvas = mk("canvas", null, this.board);
    this.canvas.setAttribute("role", "img");
    this.renderer = new Renderer(this.canvas);
    this.renderer.setTheme(this.isDark());

    const bar = mk("div", "tb-viz-bar");
    this.playBtn = mk("button", null, bar);
    this.playBtn.textContent = "▶";
    this.playBtn.title = "Play / pause (space)";
    this.playBtn.onclick = () => (this.playing ? this.pause() : this.play());

    this.slider = mk("input", null, bar);
    this.slider.type = "range";
    this.slider.min = String(this.lo);
    this.slider.max = String(this.hi);
    this.slider.value = String(this.i);
    this.slider.setAttribute("aria-label", "Turn");
    this.slider.oninput = () => {
      this.pause();
      this.seek(Number(this.slider.value));
    };

    this.turnLabel = mk("span", "tb-viz-turn", bar);

    this.speedBtn = mk("button", null, bar);
    this.speedBtn.title = "Playback speed";
    this.speedBtn.onclick = () => {
      const n = SPEEDS[(SPEEDS.indexOf(this.speed) + 1) % SPEEDS.length];
      this.speed = n;
      this.speedBtn.textContent = `${n}×`;
    };
    this.speedBtn.textContent = `${this.speed}×`;

    this.seats = mk("div", "tb-viz-seats");
    this.meta = mk("span", "tb-viz-meta");

    this.el.tabIndex = 0;
    this.el.onkeydown = (e) => this.key(e);
  }

  isDark() {
    const t = this.el.dataset.tbTheme;
    if (t) return t === "dark";
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  }

  key(e) {
    const k = e.key;
    if (k === " ") {
      e.preventDefault();
      this.playing ? this.pause() : this.play();
    } else if (k === "ArrowRight") {
      e.preventDefault();
      this.pause();
      this.seek(this.i + (e.shiftKey ? 10 : 1));
    } else if (k === "ArrowLeft") {
      e.preventDefault();
      this.pause();
      this.seek(this.i - (e.shiftKey ? 10 : 1));
    } else if (k === "Home") {
      e.preventDefault();
      this.pause();
      this.seek(this.lo);
    } else if (k === "End") {
      e.preventDefault();
      this.pause();
      this.seek(this.hi);
    }
  }

  fit() {
    const r = this.board.getBoundingClientRect();
    const h = r.height > 40 ? r.height - 24 : (this.opts.height ?? 420);
    this.renderer.resize(Math.max(40, r.width - 24), Math.max(40, h));
    this.renderer.render(this.frames[this.i]);
  }

  seek(turn) {
    this.i = clamp(turn, this.lo, this.hi);
    this.slider.value = String(this.i);
    this.show();
  }

  show() {
    const f = this.frames[this.i];
    this.renderer.setTheme(this.isDark());
    this.renderer.render(f);
    this.turnLabel.textContent = `turn ${f.turn} / ${this.frames[this.hi].turn}`;
    this.canvas.setAttribute(
      "aria-label",
      `Turn ${f.turn}: ${f.ants.length} ants, ${f.food.length} food, scores ${f.score.join(" to ")}`
    );

    const names = seatNames(this.replay);
    this.seats.innerHTML = "";
    const d = this.el.ownerDocument;
    f.score.forEach((s, seat) => {
      const row = d.createElement("span");
      row.className = "tb-viz-seat";
      const chip = d.createElement("span");
      chip.className = "tb-viz-chip";
      chip.style.background = SEATS[seat % SEATS.length];
      const label = d.createElement("span");
      label.textContent = names[seat] ?? `seat ${seat}`;
      const score = d.createElement("span");
      score.className = "tb-viz-score";
      const alive = f.ants.filter((a) => a[2] === seat).length;
      score.textContent = `${s} · ${alive} ants`;
      score.style.color = "var(--tb-dim)";
      row.append(chip, label, score);
      this.seats.appendChild(row);
    });
    this.seats.appendChild(this.meta);
    this.meta.textContent = this.i === this.hi && this.replay.reason
      ? `${this.replay.map_id ?? ""} · ${this.replay.reason}`
      : this.replay.map_id ?? "";

    if (this.onTurn) this.onTurn(f);
  }

  play() {
    if (this.playing || this.destroyed) return;
    if (this.i >= this.hi) this.seek(this.lo);
    this.playing = true;
    this.playBtn.textContent = "❚❚";
    let last = performance.now();
    let acc = 0;
    const tick = (now) => {
      if (!this.playing) return;
      acc += (now - last) * this.speed;
      last = now;
      const per = 1000 / 12; // twelve turns a second at 1x -- fast enough to read
      while (acc >= per) {
        acc -= per;
        if (this.i >= this.hi) {
          this.pause();
          return;
        }
        this.i++;
      }
      this.slider.value = String(this.i);
      this.show();
      this.raf = requestAnimationFrame(tick);
    };
    this.raf = requestAnimationFrame(tick);
  }

  pause() {
    this.playing = false;
    if (this.playBtn) this.playBtn.textContent = "▶";
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

/** Seat labels: what the replay says, else the hash, else the seat number. */
function seatNames(replay) {
  const seats = replay.seats;
  if (!Array.isArray(seats)) return [];
  const out = [];
  for (const s of seats) {
    out[s.seat ?? out.length] =
      s.label ?? (s.weights_hash ? s.weights_hash.slice(7, 15) : undefined);
  }
  return out;
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function escape(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}
