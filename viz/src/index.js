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

export { Viewer };
export { SEATS } from "./render.js";
export { frameAt, allFrames, board } from "./engine.js";
