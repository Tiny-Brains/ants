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
 * @param {object} [opts]  turn, from, to, autoplay, speed, theme ("light"|"dark"), onTurn, height
 * @returns {Promise<Viewer>}  call .destroy() when the page is done with it
 */
export async function mount(target, replay, opts = {}) {
  const el = typeof target === "string" ? document.querySelector(target) : target;
  if (!el) throw new Error(`no element for ${target}`);
  const env = typeof replay === "string" ? await (await fetch(replay)).json() : replay;
  return new Viewer(el, env, opts);
}

/**
 * Read viewer options out of a URL, so a link can point at a moment.
 *
 * `#turn=84`, `#from=40&to=60&autoplay=1`, `#turn=84&zoom=4&centre=31,72`. A replay is evidence,
 * and evidence gets cited: the turn AND the corner of the board someone wants to talk about should
 * be linkable rather than described.
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
    zoom: num("zoom"),
    theme: q.get("theme") || undefined,
  };
  const centre = q.get("centre") || q.get("center");
  if (centre && /^-?\d+,-?\d+$/.test(centre)) out.centre = centre.split(",").map(Number);
  for (const k of Object.keys(out)) if (out[k] === undefined || Number.isNaN(out[k])) delete out[k];
  return out;
}

export { Viewer };
export { SEATS } from "./render.js";
export { frameAt, allFrames, board } from "./engine.js";
