# The Ants viewer

One bundle, three consumers: the web application's Replay screen, the book's tutorials, and
`tinybrains view`. It re-simulates through **the same component digest that recorded the match** —
`replay-decode`, transpiled by `jco` and running in the browser — so the viewer and the referee
cannot disagree about what happened. There is no JavaScript re-implementation of any rule here,
and there must never be one.

## Using it

```js
// Anything that is not a React application: the book, tinybrains view, a plain page.
import { mount } from "/cartridges/ants/viz.js";
const viewer = await mount("#replay", replayEnvelope, { from: 40, to: 60, autoplay: true });
// viewer.destroy() when the page is done with it

// The web application.
import { AntsReplay } from "/cartridges/ants/react.js";
<AntsReplay replay={envelope} onTurn={(f) => setScore(f.score)} />
```

A replay carries its own board, so the envelope is the only input. `from`/`to` narrow the timeline
without renumbering it, which is how a tutorial points at turns 40–60 of a real match and the
reader still sees "turn 47".

## Building

```sh
./build.sh          # jco transpile + copy; writes dist/
```

`dist/` is committed exactly as `tb-ants.wasm` is, so a clone with no Node still runs the viewer
and only someone changing it needs the toolchain. `dist/engine.json` records the component digest
the bundle carries: a replay naming a different `engine_digest` is one this build cannot faithfully
show, and `tinybrains view` says so rather than drawing it anyway.

## Layout

```text
src/engine.js   the cartridge in the browser -- decode one frame, or a range in one pass
src/render.js   pixels: terrain, food, hills, ants. Decides nothing
src/shell.js    the viewer: timeline, playback, seek, seats, scores, keys
src/index.js    mount() -- built to dist/viz.js, the path the platform loads
src/react.js    the same viewer as a React component; React is a peer
```

## Two choices worth knowing

**The shell lives here, not in the web application.** `docs/cartridge.md` §7 used to put it there
and leave the cartridge "owning pixels". The viewer has three consumers that are not one
application, and a shell split across three of them is a shell maintained in three places. The
trade is that a second cartridge writes its own scrubber; §7 says so, and the extraction point is
whenever that cartridge exists and there are two real viewers to generalise from.

**It is framework-free, with a React wrapper.** This is a canvas, a slider and a few readouts;
React buys it nothing and would tie the cartridge to a React version. `react.js` is the only file
that mentions React at all.

## What the viewer never does

**Run a model.** It re-simulates from recorded actions — no ONNX, no adapter, no competitor code
in the browser. That is why the docs and the marketing site can embed it without inheriting the
evaluator's security surface.
