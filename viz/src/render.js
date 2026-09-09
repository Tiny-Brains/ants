// Pixels. Everything here is drawing; nothing here decides anything about the game.
//
// A frame is what `replay-decode` returns:
//
//   { turn, size: [rows, cols], water: { rle }, ants: [[r, c, owner]], food: [[r, c]],
//     hills: [[r, c, owner]], score: [], ranks: [], done }
//
// # Two things the board owes the reader
//
// **The terrain is drawn once.** Water never changes, so it is painted into an offscreen canvas at
// one pixel per cell and then scaled. That is what makes zooming and panning cost nothing: the
// per-frame work is the ants, the food and the hills, which are tens of shapes rather than sixteen
// thousand cells.
//
// **The board keeps its own colours.** A viewer's chrome follows the page's theme; the board does
// not, any more than a video changes colour with the player around it. A match looks like itself.

/// Seat colours. Two is the case that exists; the rest are here so a four-seat board is not a bug.
export const SEATS = [
  "#e2542c", // vermilion
  "#3a9bd9", // azure
  "#7fbf3f", // leaf
  "#c86fd4", // orchid
  "#f0b429", // amber
  "#46c2a8", // teal
];

const LAND = "#cdbb95";
const LAND_ALT = "#c7b48c"; // a second sand, for a very faint checker at high zoom
const WATER = "#25333d";
const WATER_EDGE = "#1b262e";
const FOOD = "#fff4d6";

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

function hexToRgb(h) {
  return [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
}

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.rows = 0;
    this.cols = 0;
    this.scale = 4; // pixels per cell
    this.ox = 0; // pan, in canvas pixels
    this.oy = 0;
    this.showGrid = true;
    // Whether the board should keep filling the viewport as it resizes. True until someone zooms
    // or pans, because until then "fit" is what they asked for and a resize should honour it.
    //
    // It is a flag rather than a comparison against `fitScale()` on purpose: the first `resize`
    // runs while the canvas is still zero-sized, so that comparison is against a fit scale of
    // zero, concludes the view was not fitted, and leaves the board at its constructor default --
    // which is how a 96x96 board opened at four pixels a cell in a 900-pixel frame.
    this.fitted = true;
  }

  /**
   * Paint the terrain once, at one pixel per cell.
   *
   * The board is the same for every frame of a match, so this is the only place the sixteen
   * thousand cells of a `cell` board are ever touched.
   */
  setBoard(map) {
    if (!map) return;
    this.rows = map.rows;
    this.cols = map.cols;
    const water = expandRle(map.water, map.rows * map.cols);

    const off = document.createElement("canvas");
    off.width = map.cols;
    off.height = map.rows;
    const g = off.getContext("2d");
    const img = g.createImageData(map.cols, map.rows);
    const [lr, lg, lb] = hexToRgb(LAND);
    const [ar, ag, ab] = hexToRgb(LAND_ALT);
    const [wr, wg, wb] = hexToRgb(WATER);
    for (let i = 0; i < water.length; i++) {
      const p = i * 4;
      const r = (i / map.cols) | 0;
      const c = i % map.cols;
      if (water[i]) {
        img.data[p] = wr;
        img.data[p + 1] = wg;
        img.data[p + 2] = wb;
      } else {
        // A whisper of a checker so the grid reads even where nothing is happening.
        const alt = (r + c) & 1;
        img.data[p] = alt ? ar : lr;
        img.data[p + 1] = alt ? ag : lg;
        img.data[p + 2] = alt ? ab : lb;
      }
      img.data[p + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    this.terrain = off;
  }

  /** Cells that fit across the viewport at the current scale. */
  viewport() {
    const dpr = window.devicePixelRatio || 1;
    return { w: this.canvas.width / dpr, h: this.canvas.height / dpr };
  }

  /** The scale at which the whole board fits. */
  fitScale() {
    const { w, h } = this.viewport();
    if (!this.rows) return 1;
    return Math.min(w / this.cols, h / this.rows);
  }

  fit() {
    this.scale = this.fitScale();
    this.fitted = true;
    this.centre();
  }

  centre() {
    const { w, h } = this.viewport();
    this.ox = (this.cols * this.scale - w) / 2;
    this.oy = (this.rows * this.scale - h) / 2;
    this.clamp();
  }

  /**
   * Keep the board in view.
   *
   * When the board is smaller than the viewport it is centred; when it is larger, panning stops at
   * its edges. The board wraps in play, but a viewer that wrapped would show the same ant twice and
   * make a position harder to read, not easier.
   */
  clamp() {
    const { w, h } = this.viewport();
    const bw = this.cols * this.scale;
    const bh = this.rows * this.scale;
    this.ox = bw <= w ? (bw - w) / 2 : Math.max(0, Math.min(this.ox, bw - w));
    this.oy = bh <= h ? (bh - h) / 2 : Math.max(0, Math.min(this.oy, bh - h));
  }

  /** Zoom about a point in canvas pixels, so the cell under the cursor stays under it. */
  zoomAt(factor, px, py) {
    const min = this.fitScale();
    const before = this.scale;
    this.scale = Math.max(min, Math.min(48, this.scale * factor));
    if (this.scale === before) return;
    this.fitted = Math.abs(this.scale - min) < 0.001;
    const k = this.scale / before;
    this.ox = (this.ox + px) * k - px;
    this.oy = (this.oy + py) * k - py;
    this.clamp();
  }

  /** Put a cell in the middle of the viewport, at the current scale. */
  centreOn(r, c) {
    const { w, h } = this.viewport();
    this.ox = (c + 0.5) * this.scale - w / 2;
    this.oy = (r + 0.5) * this.scale - h / 2;
    this.clamp();
  }

  pan(dx, dy) {
    this.ox -= dx;
    this.oy -= dy;
    this.clamp();
  }

  /** Whether the board is currently filling the viewport. */
  atFit() {
    return this.fitted;
  }

  resize(width, height) {
    const dpr = window.devicePixelRatio || 1;
    const wasFitted = this.fitted;
    this.canvas.width = Math.max(1, Math.round(width * dpr));
    this.canvas.height = Math.max(1, Math.round(height * dpr));
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (wasFitted) this.fit();
    else this.clamp();
  }

  render(frame) {
    const { ctx } = this;
    const { w, h } = this.viewport();
    ctx.clearRect(0, 0, w, h);
    if (!frame || !this.terrain) return;

    const s = this.scale;
    const x0 = -this.ox;
    const y0 = -this.oy;

    // Terrain: one scaled blit, nearest-neighbour so a cell stays a crisp square.
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(this.terrain, x0, y0, this.cols * s, this.rows * s);

    // Only what is on screen. At high zoom this is a handful of cells rather than the board.
    const c0 = Math.max(0, Math.floor(this.ox / s) - 1);
    const c1 = Math.min(this.cols, Math.ceil((this.ox + w) / s) + 1);
    const r0 = Math.max(0, Math.floor(this.oy / s) - 1);
    const r1 = Math.min(this.rows, Math.ceil((this.oy + h) / s) + 1);
    const onScreen = (r, c) => r >= r0 && r < r1 && c >= c0 && c < c1;
    const px = (c) => x0 + c * s;
    const py = (r) => y0 + r * s;

    if (this.showGrid && s >= 9) {
      ctx.strokeStyle = "rgba(0,0,0,0.07)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let c = c0; c <= c1; c++) {
        ctx.moveTo(Math.round(px(c)) + 0.5, py(r0));
        ctx.lineTo(Math.round(px(c)) + 0.5, py(r1));
      }
      for (let r = r0; r <= r1; r++) {
        ctx.moveTo(px(c0), Math.round(py(r)) + 0.5);
        ctx.lineTo(px(c1), Math.round(py(r)) + 0.5);
      }
      ctx.stroke();
    }

    // Hills first: an ant standing on one has to be visible on top of it, because "who is sitting
    // on whose hill" is usually the thing being read.
    // A SQUARE ring, where an ant is a circle. An ant starts the match standing on its own hill and
    // spends much of the match near it, so the two are almost always drawn on the same cell -- and
    // a hill that was also a circle simply disappeared under the ant. Different shapes read at a
    // glance even when one is on top of the other.
    for (const [r, c, owner] of frame.hills) {
      if (!onScreen(r, c)) continue;
      const col = SEATS[owner % SEATS.length];
      const x = px(c);
      const y = py(r);
      ctx.fillStyle = col;
      ctx.globalAlpha = 0.3;
      ctx.fillRect(x, y, s, s);
      ctx.globalAlpha = 1;
      const lw = Math.max(1.5, s * 0.16);
      ctx.lineWidth = lw;
      ctx.strokeStyle = col;
      ctx.strokeRect(x + lw / 2, y + lw / 2, s - lw, s - lw);
    }

    for (const [r, c] of frame.food) {
      if (!onScreen(r, c)) continue;
      const x = px(c) + s / 2;
      const y = py(r) + s / 2;
      ctx.beginPath();
      ctx.arc(x, y, Math.max(0.8, s * 0.26), 0, Math.PI * 2);
      ctx.fillStyle = FOOD;
      ctx.fill();
      if (s >= 6) {
        ctx.lineWidth = 1;
        ctx.strokeStyle = "rgba(120,90,40,0.45)";
        ctx.stroke();
      }
    }

    for (const [r, c, owner] of frame.ants) {
      if (!onScreen(r, c)) continue;
      const x = px(c) + s / 2;
      const y = py(r) + s / 2;
      const rad = Math.max(1, s * 0.38);
      ctx.beginPath();
      ctx.arc(x, y, rad, 0, Math.PI * 2);
      ctx.fillStyle = SEATS[owner % SEATS.length];
      ctx.fill();
      // A dark rim at readable sizes: an ant on its own colour of hill would otherwise vanish.
      if (s >= 5) {
        ctx.lineWidth = Math.max(1, s * 0.08);
        ctx.strokeStyle = "rgba(0,0,0,0.45)";
        ctx.stroke();
      }
    }

    // The water's edge, drawn last and only when zoomed in, so the coastline reads as a coastline.
    if (s >= 14) {
      ctx.strokeStyle = WATER_EDGE;
      ctx.lineWidth = 1;
      ctx.strokeRect(x0 + 0.5, y0 + 0.5, this.cols * s - 1, this.rows * s - 1);
    }
  }

  /** Which cell a canvas point is over, or null. */
  cellAt(px, py) {
    const c = Math.floor((px + this.ox) / this.scale);
    const r = Math.floor((py + this.oy) / this.scale);
    if (r < 0 || c < 0 || r >= this.rows || c >= this.cols) return null;
    return [r, c];
  }
}
