// The viewer, as the platform loads it: `/cartridges/ants/viz.js`.
//
// Three consumers, one bundle. `mount` is for anything that is not a React application -- the
// book's tutorials and `tinybrains view`; `react.js` beside this wraps the same viewer as a
// component for the web application. Neither re-implements a rule: both drive `replay-decode` in
// the transpiled component, which is the same digest that recorded the match.

import { Viewer } from "./shell.js";

export const meta = { gameId: "ants", abiVersion: 1 };

/**
 * Draw a replay into an element.
 *
 * @param {HTMLElement|string} target   an element, or a selector
 * @param {object|string} replay        the envelope, or a URL to fetch it from
 * @param {object} [opts]  turn, from, to, autoplay, speed, theme ("light"|"dark"),
 *                         chrome ("hover"|"always"), height (the player's), stageHeight (the
 *                         board's -- the player is that plus its bars; wins over height), onTurn,
 *                         labels ([{ seat, name, by }] -- what the host calls each seat),
 *                         explored (open with each seat's explored territory drawn)
 * @returns {Promise<Viewer>}  call .destroy() when the page is done with it
 */
export async function mount(target, replay, opts = {}) {
  const el = typeof target === "string" ? document.querySelector(target) : target;
  if (!el) throw new Error(`no element for ${target}`);
  const env = typeof replay === "string" ? await (await fetch(replay)).json() : replay;
  return new Viewer(el, env, opts);
}

/**
 * Draw a board on its own -- the map visual. No seats, no transport, no tray: the board at turn
 * zero, through the cartridge, under its name, its player count and its size in cells.
 *
 * @param {HTMLElement|string} target   an element, or a selector
 * @param {object|string} board         the map file, whole, or a URL to fetch it from
 * @param {object} [opts]  name (instead of the board's own id), theme ("light"|"dark"), maxHeight
 * @returns {Promise<MapView>}  call .destroy() when the page is done with it
 */
export async function mountMap(target, board, opts = {}) {
  const el = typeof target === "string" ? document.querySelector(target) : target;
  if (!el) throw new Error(`no element for ${target}`);
  const map = typeof board === "string" ? await (await fetch(board)).json() : board;
  // Loaded when a board is first drawn, not with the viewer. A host that copies the viewer's
  // modules by name -- web's Dockerfile does -- keeps every replay working with a list that
  // predates this file, rather than losing viz.js to one import it cannot resolve.
  const { MapView } = await import("./map.js");
  return new MapView(el, map, opts);
}

/**
 * Read viewer options out of a URL, so a link can point at a moment.
 *
 * `#turn=84`, `#from=40&to=60&autoplay=1`, `#turn=84&zoom=4&centre=31,72`. A replay is evidence,
 * and evidence gets cited: the turn AND the corner of the board someone wants to talk about should
 * be linkable rather than described.
 *
 * `#chrome=always` pins the tray of readouts open, for a screenshot or a page where the viewer is
 * not the thing being hovered. `#explored=1` opens with each seat's explored territory drawn.
 */
export function optsFromHash(url = location) {
  const q = new URLSearchParams((url.hash || "").replace(/^#/, "") || url.search || "");
  const num = (k) => (q.has(k) ? Number(q.get(k)) : undefined);
  const out = {
    turn: num("turn"),
    from: num("from"),
    to: num("to"),
    speed: num("speed"),
    autoplay: q.get("autoplay") === "1" || q.get("autoplay") === "true",
    explored: q.get("explored") === "1" || q.get("explored") === "true",
    zoom: num("zoom"),
    theme: q.get("theme") || undefined,
    chrome: q.get("chrome") || undefined,
  };
  const centre = q.get("centre") || q.get("center");
  if (centre && /^-?\d+,-?\d+$/.test(centre)) out.centre = centre.split(",").map(Number);
  for (const k of Object.keys(out)) if (out[k] === undefined || Number.isNaN(out[k])) delete out[k];
  return out;
}

export { Viewer };
export { SEATS } from "./render.js";
export { frameAt, allFrames, board } from "./engine.js";
