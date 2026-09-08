// Pixels. Everything in this file is drawing; nothing in it decides anything about the game.
//
// A frame is what `replay-decode` returns:
//
//   { turn, size: [rows, cols], water: { rle }, ants: [[r, c, owner]], food: [[r, c]],
//     hills: [[r, c, owner]], score: [], ranks: [], done }
//
// The grid wraps, but a viewer does not: drawing the torus flat is what lets someone read a
// position at a glance, and the wrap is a fact about movement rather than about the picture.

/** Seat colours. Two are the case that exists; the rest are here so a 4-seat board is not a bug. */
export const SEATS = ["#d1552b", "#2b7f95", "#7a9c3f", "#9a5fa8", "#c99a22", "#4a63b8"];

const THEME = {
  light: { land: "#efeee7", water: "#c3c9c6", grid: "#e4e3db", food: "#3f7d3f", edge: "#b9b8ad" },
  dark: { land: "#20231f", water: "#12211f", grid: "#282b26", food: "#67a95f", edge: "#33362f" },
};

/** Expand `[value, run, value, run, ...]` into a row-major flag array. */
function expandRle(rle, cells) {
  const out = new Uint8Array(cells);
  let i = 0;
  for (let k = 0; k + 1 < rle.length; k += 2) {
    const on = rle[k] === 1;
    const run = rle[k + 1];
    if (on) out.fill(1, i, Math.min(i + run, cells));
    i += run;
  }
  return out;
}

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.water = null;
    this.dark = false;
    this.showGrid = false;
  }

  /** Terrain never changes during a match, so it is expanded once rather than per frame. */
  setBoard(map) {
    if (!map) return;
    this.rows = map.rows;
    this.cols = map.cols;
    this.water = expandRle(map.water, map.rows * map.cols);
  }

  setTheme(dark) {
    this.dark = dark;
  }

  /** Fit the board to the element, on whole pixels so a cell never straddles one. */
  resize(width, height) {
    if (!this.rows) return;
    const dpr = window.devicePixelRatio || 1;
    const cell = Math.max(1, Math.floor(Math.min(width / this.cols, height / this.rows)));
    this.cell = cell;
    const w = cell * this.cols;
    const h = cell * this.rows;
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this.canvas.style.width = `${w}px`;
    this.canvas.style.height = `${h}px`;
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  render(frame) {
    if (!frame || !this.cell) return;
    const { ctx, cell } = this;
    const t = this.dark ? THEME.dark : THEME.light;
    const w = cell * this.cols;
    const h = cell * this.rows;

    ctx.fillStyle = t.land;
    ctx.fillRect(0, 0, w, h);

    // Water, cell by cell. A run-oriented fill would be faster and is not needed: the largest
    // board is 128x128 and this is one paint per animation frame.
    ctx.fillStyle = t.water;
    if (this.water) {
      for (let i = 0; i < this.water.length; i++) {
        if (this.water[i]) {
          ctx.fillRect(((i % this.cols) | 0) * cell, ((i / this.cols) | 0) * cell, cell, cell);
        }
      }
    }

    if (this.showGrid && cell >= 6) {
      ctx.strokeStyle = t.grid;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let c = 1; c < this.cols; c++) {
        ctx.moveTo(c * cell + 0.5, 0);
        ctx.lineTo(c * cell + 0.5, h);
      }
      for (let r = 1; r < this.rows; r++) {
        ctx.moveTo(0, r * cell + 0.5);
        ctx.lineTo(w, r * cell + 0.5);
      }
      ctx.stroke();
    }

    // Hills first: an ant standing on one must be visible on top of it, because "who is sitting on
    // whose hill" is the thing a viewer is usually trying to read.
    for (const [r, c, owner] of frame.hills) {
      const x = c * cell;
      const y = r * cell;
      ctx.fillStyle = SEATS[owner % SEATS.length];
      ctx.globalAlpha = 0.35;
      ctx.fillRect(x, y, cell, cell);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = SEATS[owner % SEATS.length];
      ctx.lineWidth = Math.max(1, cell * 0.18);
      ctx.strokeRect(x + cell * 0.12, y + cell * 0.12, cell * 0.76, cell * 0.76);
    }

    ctx.fillStyle = t.food;
    const fr = Math.max(1, cell * 0.3);
    for (const [r, c] of frame.food) {
      ctx.beginPath();
      ctx.arc(c * cell + cell / 2, r * cell + cell / 2, fr, 0, Math.PI * 2);
      ctx.fill();
    }

    for (const [r, c, owner] of frame.ants) {
      ctx.fillStyle = SEATS[owner % SEATS.length];
      const inset = cell <= 3 ? 0 : cell * 0.14;
      ctx.fillRect(c * cell + inset, r * cell + inset, cell - inset * 2, cell - inset * 2);
    }

    ctx.strokeStyle = t.edge;
    ctx.lineWidth = 1;
    ctx.strokeRect(0.5, 0.5, w - 1, h - 1);
  }
}
