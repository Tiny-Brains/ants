// The map visual: a board on its own.
//
// A season's map page, an admin's list of uploads and the book's board pages want a BOARD, not a
// match, and the player around a match -- the seats' title bar, the transport, the tray -- is noise
// there: a timeline with one frame on it, a play button that plays nothing, seat names nobody has.
// So this draws the board and exactly three facts over it: its name, how many play it, and its size
// in cells. Nothing is clickable, nothing appears on hover, and there is no keyboard.
//
// **Turn zero comes from the cartridge, never from reading the file here.** Which seat owns which
// hill, and what food a match opens on, are the engine's to say: the board goes through
// `replay-decode` as an envelope of no moves, exactly as a replay would, so a board the engine
// refuses is shown as refused rather than drawn as if it were playable. Of that frame only the
// terrain, the hills and the food are drawn -- the ants a match opens with are the match's, and on
// a board they would sit on every hill and hide it.
//
// It shares the viewer's one stylesheet (shell.js, scoped to `.tb-viz` and checked by check.mjs)
// and its renderer, so a board here and the same board in a replay are the same pixels.

import { frameAt } from "./engine.js";
import { Renderer } from "./render.js";
import { injectCss } from "./shell.js";

// Two figures, as the platform draws them elsewhere: seats as people, size as a grid of cells.
const ICON = {
  players: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="6" cy="5.2" r="2.4"/><path d="M1.8 13.5c.5-2.6 2.2-4 4.2-4s3.7 1.4 4.2 4"/><circle cx="11.4" cy="5.8" r="1.9"/><path d="M11.6 9.6c1.5.2 2.5 1.4 2.8 3.4"/></svg>',
  cells: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="2.2" y="2.2" width="11.6" height="11.6" rx="1.2"/><path d="M6.1 2.2v11.6M9.9 2.2v11.6M2.2 6.1h11.6M2.2 9.9h11.6"/></svg>',
};

/**
 * How big to draw a board in a host of a given width: the full width, and the height its shape
 * asks for, unless that is taller than the host allows -- then the height is capped and the board
 * is centred in it. Whole pixels, and never below one, so a host still measuring itself as zero
 * wide draws nothing rather than NaN.
 *
 * @returns {{ w: number, h: number }}
 */
export function mapFrame(width, rows, cols, maxHeight = Infinity) {
  const w = Math.max(1, Math.floor(width));
  if (!rows || !cols) return { w, h: 1 };
  const h = Math.max(1, Math.min(Math.round((w * rows) / cols), Math.floor(maxHeight)));
  return { w, h };
}

export class MapView {
  /**
   * @param {HTMLElement} el   where to draw
   * @param {object} board     the map file, whole, exactly as a season's upload or a release carries it
   * @param {object} [opts]    { name, theme, maxHeight }: `name` overrides the board's own `id`,
   *                           `maxHeight` caps how tall a tall board may draw
   */
  constructor(el, board, opts = {}) {
    this.el = el;
    this.board = board;
    this.opts = opts;
    this.destroyed = false;

    injectCss(el.ownerDocument);
    el.innerHTML = "";
    el.classList.add("tb-viz");
    el.dataset.tbMode = "map";
    if (opts.theme) el.dataset.tbTheme = opts.theme;

    const rows = Number(board?.rows) || 0;
    const cols = Number(board?.cols) || 0;
    const players = Number(board?.players) || 0;
    const name = String(opts.name ?? board?.id ?? "");
    this.rows = rows;
    this.cols = cols;

    const d = el.ownerDocument;
    const head = d.createElement("div");
    head.className = "tb-map-head";
    const fact = (icon, text, title) =>
      `<span class="tb-map-fact" title="${esc(title)}">${icon}<span>${esc(text)}</span></span>`;
    head.innerHTML =
      `<span class="tb-map-name" title="${esc(name)}">${esc(name)}</span>` +
      fact(ICON.players, String(players), `${players} ${players === 1 ? "player" : "players"}`) +
      fact(ICON.cells, `${rows} × ${cols}`, `${rows} rows × ${cols} columns`);
    el.appendChild(head);

    try {
      // Turn zero, through the cartridge. No deltas, so nothing is re-simulated past the opening.
      const opening = frameAt({ seed: 1, max_turns: 1, turns: 0, map: board, deltas: [] }, 0);
      this.frame = { ...opening, ants: [] };
    } catch (e) {
      const err = d.createElement("div");
      err.className = "tb-err";
      err.innerHTML = `This board could not be drawn.<br>${esc(e.message)}`;
      el.appendChild(err);
      return;
    }

    this.stage = d.createElement("div");
    this.stage.className = "tb-map-board";
    this.canvas = d.createElement("canvas");
    this.canvas.setAttribute("role", "img");
    this.canvas.setAttribute("aria-label", `${name}: ${players} players, ${rows} by ${cols} cells`);
    this.stage.appendChild(this.canvas);
    el.appendChild(this.stage);

    this.renderer = new Renderer(this.canvas);
    this.renderer.setBoard(board);
    this.observe();
  }

  observe() {
    const fit = () => {
      if (this.destroyed) return;
      const { w, h } = mapFrame(this.el.clientWidth, this.rows, this.cols, this.opts.maxHeight);
      if (w < 2) return;
      this.stage.style.height = `${h}px`;
      this.renderer.resize(w, h);
      this.renderer.fit();
      this.renderer.render(this.frame);
    };
    this.ro = new ResizeObserver(fit);
    this.ro.observe(this.el);
    fit();
  }

  destroy() {
    this.destroyed = true;
    if (this.ro) this.ro.disconnect();
    this.el.innerHTML = "";
    this.el.classList.remove("tb-viz");
    delete this.el.dataset.tbMode;
  }
}

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}
