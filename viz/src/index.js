// The viewer, as the platform loads it: `/cartridges/ants/viz.js`.
//
// Three consumers, one bundle. `mount` is for anything that is not a React application -- the
// book's tutorials and `tinybrains view`; `react.js` beside this wraps the same viewer as a
// component for the web application. Neither re-implements a rule: both drive `replay-decode` in
// the transpiled component, which is the same digest that recorded the match.
//
// The static imports here are a contract: web copies `viz.js shell.js render.js engine.js` and the
// transpiled component by name. `map.js` and `graph.js` are on that list too, but loaded on first
// use, so a host whose list predates one of them loses that one visual and keeps every replay.

import { Viewer } from "./shell.js";

export const meta = { gameId: "ants", abiVersion: 1 };

/**
 * Draw a replay into an element, at one of four tiers.
 *
 *   stage    (the default) seat cards with ants and hills, the board with its tray, the transport
 *            with speed, the key list and fullscreen
 *   player   seat cards with name, owner and score, the board, the transport and fullscreen, which
 *            promotes it to a stage in place
 *   tile     the board in a 16:10 box, names and scores over it, the turn in a corner; nothing to
 *            operate. `preview()` plays the last forty turns and `stop()` rests it
 *   thumb    one frame of the board, and a score chip from 160 pixels wide
 *
 * A tile or a thumb may be given `replay: null` with `opts.frame` (a match's last frame, as
 * `GET /v1/matches/{id}/frame` serves it in `.frame`) or `opts.board` (the map file, for turn
 * zero). A tile given no envelope must be given `labels`: nothing else names its seats.
 *
 * @param {HTMLElement|string} target   an element, or a selector
 * @param {object|string|null} replay   the envelope, or a URL to fetch it from, or null (above)
 * @param {object} [opts]  tier ("stage"|"player"|"tile"|"thumb"), frame, board, turn, from, to,
 *                         autoplay, speed, theme ("light"|"dark"), chrome ("hover"|"always"),
 *                         height (the player's), stageHeight (the board's -- the player is that
 *                         plus its bars; wins over height), onTurn, labels ([{ seat, name, by }] --
 *                         what the host calls each seat), explored (open with each seat's explored
 *                         territory drawn)
 * @returns {Promise<Viewer>}  call .destroy() when the page is done with it
 */
export async function mount(target, replay, opts = {}) {
  const el = element(target);
  const env = typeof replay === "string" ? await (await fetch(replay)).json() : replay;
  return new Viewer(el, env ?? null, opts);
}

/**
 * Draw one stored frame -- a match's last, as Soma keeps it -- as a thumb (or, with
 * `opts.tier: "tile"`, a tile). Synchronous, and it never calls the component: the frame carries
 * its own size and water, so there is nothing to decode.
 *
 * @param {HTMLElement|string} target   an element, or a selector
 * @param {object} frame   a `replay-decode` frame: `{ turn, size, water: { rle }, ants, food, hills,
 *                         score, ... }`
 * @param {object} [opts]  tier ("thumb"|"tile"), labels, theme, onTurn
 * @returns {Viewer}  call .destroy() when the page is done with it
 */
export function drawFrame(target, frame, opts = {}) {
  const tier = opts.tier === "tile" ? "tile" : "thumb";
  return new Viewer(element(target), null, { ...opts, tier, frame });
}

/**
 * The ants graph: each seat's ants alive, hills standing or score over the match, one line a seat,
 * under a viewer and following it. Its x-axis is the viewer's timeline, a playhead marks the turn
 * shown, and hovering it scrubs the viewer.
 *
 * @param {HTMLElement|string} target   an element, or a selector
 * @param {Viewer} viewer   a stage or a player
 * @param {object} [opts]  kind ("ants"|"hills"|"score", the series it opens on), height (the
 *                         plot's, 140 pixels by default), theme
 * @returns {Promise<Graph>}  call .destroy() when the page is done with it
 */
export async function mountGraph(target, viewer, opts = {}) {
  const el = element(target);
  // Loaded on first use, as map.js is: see the note at the top of this file.
  const { Graph } = await import("./graph.js");
  return new Graph(el, viewer, opts);
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
  const el = element(target);
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

function element(target) {
  const el = typeof target === "string" ? document.querySelector(target) : target;
  if (!el) throw new Error(`no element for ${target}`);
  return el;
}

export { Viewer };
export { SEATS } from "./render.js";
export { frameAt, allFrames, board } from "./engine.js";
