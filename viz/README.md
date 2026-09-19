# The Ants viewer

One bundle for three consumers: the web application's replay and map pages, the book, and
`tinybrains view`. It re-simulates a match through the component that recorded it (`replay-decode`,
transpiled by `jco` and running in the browser), so the viewer and the referee cannot disagree
about what happened. It ships in the release archive and in `dist/viz/`; web serves it at
`/cartridges/ants/`.

## Usage

```js
// The web application, the book, tinybrains view: any page.
import { mount } from "/cartridges/ants/viz.js";
const viewer = await mount("#replay", replayEnvelope, { from: 40, to: 60, autoplay: true });
// viewer.destroy() when the page is done with it

// A React host can use the wrapper instead. React is a peer dependency.
import { AntsReplay } from "./react.js";
<AntsReplay replay={envelope} onTurn={(f) => setScore(f.score)} />
```

The web application imports `viz.js` directly and serves only the modules it needs, not `react.js`.

`mount(target, replay, opts)` takes an element or a selector, and the envelope or a URL to fetch it
from. A replay carries its own board, so the envelope is the only input. `viz.js` also exports
`mountMap`, `optsFromHash`, `Viewer`, `SEATS`, `frameAt`, `allFrames`, `board` and `meta`.

The player is two bars and a board: the seats are a title bar above it (colour, model name and
owner, then ants, hills and score), and the transport a bar below it. Seats lay out in a grid whose
columns come from the seat count and the width alone (`seatColumns()`), so nothing a turn changes
can move the board. The zoom buttons and the territory toggle sit in a tray over the board's
top-right corner, hidden until hovered, focused or touched.

| Control | Does |
|---|---|
| Transport | first, previous, play/pause, next, last |
| Timeline | click to jump, drag to scrub; ticks mark a hill razed or a colony wiped out, in the seat's colour |
| Zoom | wheel about the cursor, drag to pan, −/+/fit buttons; opens fitted and stays fitted through a resize until zoomed |
| Hill rings | a hill with an enemy ant within eight moves (round water, across the wrap) is ringed in its owner's colour |
| Territory | the eye button or `E`: fog where nobody has looked, each seat's explored area and frontier, its share on its chip |
| Keys | `space` play/pause · `←` `→` step (shift for ten) · `↑` `↓` ten · `Home` `End` · `+` `−` `0` zoom · `E` territory |

The root has `contain: inline-size`, so the viewer takes the width its host gives it. A host that
sizes to its content (an inline-block, a float) must give it a width.

## Options and hash parameters

| Option | Meaning |
|---|---|
| `turn` | The turn to open on |
| `from`, `to` | Narrow the timeline without renumbering it: a tutorial shows turns 40–60 and the reader still sees "turn 47" |
| `autoplay`, `speed` | Start playing, and how fast |
| `zoom`, `centre` | Open zoomed about `[row, col]` |
| `theme` | `"light"` or `"dark"`; overrides the page |
| `chrome` | `"hover"` (default) or `"always"`, which pins the tray open |
| `height` | The whole player's height |
| `stageHeight` | The board's height; the player is that plus its bars. Wins over `height` |
| `labels` | `[{ seat, name, by }]`: what the host calls each seat. Without it a seat shows the envelope's name |
| `explored` | Open with territory drawn |
| `onTurn` | Called with each frame shown |

`AntsReplay` takes `replay` and the same options except `height`, `zoom` and `centre`, plus `style`
and `className`.

`optsFromHash(url = location)` reads the linkable options from a URL's hash (or query), so a link can
cite a moment and a corner of the board: `#turn=84`, `#from=40&to=60&autoplay=1`,
`#turn=84&zoom=4&centre=31,72`, `#chrome=always`, `#explored=1`. It reads `turn`, `from`, `to`,
`speed`, `autoplay`, `explored`, `zoom`, `centre` (or `center`), `theme` and `chrome`.

**Theme.** Every colour of the frame is a platform design token with a written-out fallback
(`var(--ink, #142642)`), so on a page that loads `design-system/tokens.css` the player follows the
page's theme with nothing to wire up, and elsewhere it answers `prefers-color-scheme`. The book
passes `theme` because mdBook's switch is not the operating system's. The board's palette is fixed
in both themes: a match looks like itself.

## Map visual

A board on its own, for a season's map page, an admin's list of uploads and the book's board pages:
the board at turn zero under its name, how many play it and its size in cells. No seats, no
transport, no tray; nothing to click, hover or zoom.

```js
import { mountMap } from "/cartridges/ants/viz.js";
const view = await mountMap("#board", mapFile, { maxHeight: 520 });   // the map file, whole, or a URL
// view.destroy() when the page is done with it

import { AntsMap } from "./react.js";
<AntsMap board={mapFile} />
```

Options: `name` (instead of the board's own `id`), `theme`, `maxHeight`. It takes the host's width
and the height the board's shape asks for (`mapFrame()`), capped at `maxHeight`. Turn zero comes
from the cartridge (`replay-decode` over an envelope of no moves), so a board the engine refuses is
shown as refused. It draws the terrain, the hills and the food, not the opening ants. `map.js` is
loaded by `mountMap` on first use, not with `viz.js`. The book's slots take it with
`data-view="map"`.

## Build and checks

```sh
./build.sh          # jco transpile, copy, then check.mjs; writes ../dist/viz/ (needs ../build.sh first)
node check.mjs      # just the checks
```

`check.mjs` checks what needs no browser: fitting a board to a frame, zooming about a point,
clamping a pan, turning a click back into a cell, hill-ring step counts, territory, seat labels, the
map visual's sizing and turn zero on every basic board, and a replay of
`engine/src/tests/fixtures/replay-maze-03.json` through the geometry. It fails if a stylesheet rule
is not scoped to `.tb-viz`, if the tray is not hidden until hovered, or if `.tb-viz[hidden]` is
missing (a host hides the player with it while it fetches a replay). It imports the built shell,
because a backtick inside a CSS comment ends the template literal and `node --check` does not notice.

`engine.json` records the component digest the bundle was transpiled from. A replay naming a
different `engine_digest` is one this build cannot faithfully show, and `tinybrains view` says so.

## Layout

```text
src/index.js    mount(), mountMap(), optsFromHash() -- built to viz.js, the path the platform loads
src/shell.js    the player: title bar, transport, timeline and its marks, the tray, the stylesheet,
                and what is drawn over the board (ring step counts, the territory's fold)
src/render.js   pixels: terrain, territory, rings, food, hills, ants, and the zoom/pan geometry
src/engine.js   the cartridge in the browser: decode one frame, or a range in one pass
src/map.js      the map visual: a board on its own at turn zero
src/react.js    AntsReplay and AntsMap, the same viewer as React components (web does not use them)
check.mjs       checks that need no browser
build.sh        the build into ../dist/viz/
```

## Invariants

- **No rule is re-implemented here.** Territory comes from the engine: each `replay-decode` frame
  says what each seat saw for the first time, and the shell only folds those. Ring step counts are
  computed in the shell because they decide nothing about the match.
- **The viewer never runs a model.** It re-simulates recorded actions: no ONNX, no adapter, no
  competitor code in the browser.
- **The stylesheet cannot leave the viewer.** One `<style>` goes into the host document on mount,
  not a shadow root, so every rule starts at `.tb-viz` and `check.mjs` enforces it.
- **The module list is a contract with web.** Web's `Dockerfile` and `scripts/vendor-viewers.sh`
  copy `viz.js shell.js render.js engine.js map.js` and the transpiled component by name. A new
  static import from `viz.js` is a change to both lists; that is why `map.js` loads on first use.
- **The bundle and the component travel together.** Neither is committed; both land in `dist/` and
  ship in one release, and `engine.json` names the digest the bundle carries.
- **`react.js` is the only file that mentions React.**
