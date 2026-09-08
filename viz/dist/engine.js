// The cartridge, in the browser.
//
// `replay-decode` runs here from the same component digest the referee used, which is the whole
// reason the determinism law exists: the viewer and the referee cannot disagree about what
// happened, because they are the same code. There is no JavaScript re-implementation of any rule
// in this directory, and there must never be one.
//
// The component is loaded by jco's transpiled module, which awaits its own instantiation at the
// top level -- so importing this module is enough, and `invoke` is synchronous afterwards.

import { functions } from "./engine/tb-ants.js";

const FN = "tb.ants.replay-decode";

function call(input) {
  let out;
  try {
    out = functions.invoke(FN, JSON.stringify(input));
  } catch (e) {
    // A PluginError is a refusal the cartridge chose to make, with a stable code. Anything else
    // is a trap, and reads as one rather than being flattened into "something went wrong".
    const code = e && e.payload && e.payload.code;
    throw new Error(code ? `${code}: ${e.payload.message}` : `the cartridge trapped: ${e}`);
  }
  return JSON.parse(out);
}

/** One frame, by turn. */
export function frameAt(replay, turn) {
  return call({ payload: replay, turn }).frame;
}

/**
 * Every frame of a replay, in one pass.
 *
 * This is why `replay-decode` takes a range. Decoding re-simulates from turn zero, so a scrubber
 * asking for each frame in turn would replay the match once per frame -- half a million turn-steps
 * to scrub a thousand-turn match, quadratic in exactly the interaction a timeline is made of.
 */
export function allFrames(replay) {
  const to = Number(replay.turns ?? 0);
  return call({ payload: replay, from: 0, to }).frames;
}

/** The board, without instantiating anything: it is in the envelope, in readable form. */
export function board(replay) {
  return replay.map ?? null;
}
