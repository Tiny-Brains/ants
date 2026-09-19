# The Ants viewer

One bundle, three consumers: the web application's Replay screen, the book's tutorials, and
`tinybrains view`. It re-simulates through **the same component digest that recorded the match** —
`replay-decode`, transpiled by `jco` and running in the browser — so the viewer and the referee
cannot disagree about what happened. There is no JavaScript re-implementation of any rule here,
and there must never be one.

## The player

It behaves like a media player, because watching a match is what it is for.

| | |
|---|---|
| **Transport** | first · previous · play/pause · next · last, all clickable, all with keys. The only thing always on screen |
| **Timeline** | click anywhere to jump, drag to scrub. Coloured ticks mark the turns worth finding — a hill razed, a colony wiped out — in the seat's own colour |
| **Title bar** | every seat, always on screen: its colour, its model name and whose it is, then its ants, its hills and its score — the counts in the board's own shapes, a dot and a square, and its share explored beside an eye while territory is on. The owner gives way first and whole, then the name; the numbers never do. Both are in full on hover |
| **The tray** | zoom buttons and the territory toggle, over the board's top-right corner and out of the way until you hover it, focus it or touch it |
| **Zoom** | wheel to zoom about the cursor, drag to pan, buttons for −/+/fit. The board opens fitted and stays fitted through a resize until you zoom |
| **Hill rings** | a hill with an enemy ant within eight moves of it is ringed in its owner's colour — the seat about to lose it — and the ring warms as the attacker closes. Moves, not distance: round water and across the wrap |
| **Territory** | the eye button, or `E`, draws what each seat has explored: fog where nobody has looked, each seat's colour where it has, each seat's frontier traced, and its share of the board on its chip |
| **Keys** | `space` play/pause · `←` `→` step (hold shift for ten) · `↑` `↓` ten · `Home` `End` · `+` `−` `0` zoom · `E` territory |

**Two bars and a board.** The seats are a title bar above the board and the transport a bar below
it, and both are always on screen — who is playing and what the score is are read the whole way
through a match, so they are not something to go and find. Two seats cost the board one line, about
34 pixels of a 460-pixel frame; it used to be a strip of wrapping chips that took a fifth of it.
Four seats and six do not fit one line of the web's home-page frame at any size of type, so the
seats are a grid whose columns come from the seat count and the width alone (`seatColumns()`): two
rows of two there, or of three, and one row on the match page. The number of rows never depends on
the text in them, so nothing a turn changes can move the board. The tools are the only layer over
the stage, the way a video player's chrome is, and `chrome: "always"` pins them open. The board's
identity and the cell readout were taken out on 11 September 2026.

**It takes the width its host gives it.** The root has `contain: inline-size`. The title bar is one
line of text and the canvas is drawn at the pixels it was last given, and without containment
either became the viewer's minimum width: the day the title bar arrived, the web application's
home-page replay grew from its 410-pixel column to 871 and squeezed the headline beside it into
280. A host that sizes to its content — an inline-block, a float — has to give the viewer a width.

## Light and dark

**The chrome follows the page.** Every colour of the frame is one of the platform's design tokens
with a written-out fallback — `var(--ink, #142642)` and the rest — so on a page that loads
`design-system/tokens.css` the player is the colour of the card it sits in and follows the theme
switch with nothing to wire up, and on a page that does not (the book, `tinybrains view`) it still
looks like TinyBrains and answers `prefers-color-scheme`. Pass `theme: "light" | "dark"` to override
the page; the book does, because mdBook's theme switch is not the operating system's.

**The board does not.** Its palette is fixed in both themes: a match has to look like itself, the
way a video does not change colour with the player around it. It is drawn from the platform's own
blues so it belongs here, but every value is a literal, and the tray's panels are a flat dark that
belongs to the board rather than to the theme — they are read against terrain, not against the page.

**The stylesheet cannot leave the viewer.** One `<style>` goes into the host document on mount; it
is not a shadow root. Every rule in it starts at `.tb-viz`, and `check.mjs` fails the build if one
does not — an unscoped `.tb-bar` in here once landed on the web application's own header and
silently relaid it out the moment a replay mounted.

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

`opts` in full: `turn`, `from`, `to`, `autoplay`, `speed`, `zoom`, `centre`, `theme`, `chrome`,
`height`, `labels`, `explored`, `onTurn`. `optsFromHash()` reads all of the linkable ones out of a
URL — `#turn=84`, `#from=40&to=60&autoplay=1`, `#turn=84&zoom=4&centre=31,72`, `#chrome=always`,
`#explored=1` — because a replay is evidence and evidence gets cited by turn and by corner of the
board, not described.

`labels` is what the host calls each seat: `[{ seat, name, by }]`, shown as given. An envelope only
has the referee's name for a seat — a weights hash, or a label a local run chose — so without it the
tray said `6fae1212` while every other panel on the page said `mover` `by @someone`. The web
application passes the model's name and `@owner`; the book and `tinybrains view` pass nothing and
get the envelope's.

## The map visual

A board on its own, for a season's map page, an admin's list of uploads and the book's board pages:
**the board at turn zero under exactly three facts -- its name, how many play it, and its size in
cells** -- and nothing else. No seats' title bar, no transport, no tray; nothing to click, hover or
zoom. A board is read, not played.

```js
import { mountMap } from "/cartridges/ants/viz.js";
const view = await mountMap("#board", mapFile, { maxHeight: 520 });   // the map file, whole, or a URL
// view.destroy() when the page is done with it

import { AntsMap } from "/cartridges/ants/react.js";
<AntsMap board={mapFile} />
```

`opts`: `name` (instead of the board's own `id`), `theme`, `maxHeight`. It takes the host's width and
the height the board's shape asks for (`mapFrame()`), capped at `maxHeight`, so a list of boards is
a list of their own shapes rather than letterboxed frames. **Turn zero comes from the cartridge**:
the board goes through `replay-decode` as an envelope of no moves, so which seat owns which hill and
what food it opens on are the engine's to say, and a board the engine refuses is shown as refused.
Of that frame it draws the terrain, the hills and the food -- not the opening ants, which are the
match's and would hide every hill. `map.js` is loaded by `mountMap` on first use rather than with
`viz.js`, so a host that copies the viewer's modules by name keeps its replays working with a list
that predates it. The book's slots take it with `data-view="map"`.

## Building

```sh
./build.sh          # jco transpile, geometry checks, copy; writes ../dist/viz/ (needs ../build.sh first)
node check.mjs      # just the checks
```

`check.mjs` is what can be checked without eyes: fitting a board to a frame, zooming about a point,
clamping a pan, turning a click back into a cell. Arithmetic that is out by one looks almost right
on a screen and is never noticed — the first version of the fit logic opened a 96×96 board at four
pixels a cell in a 900-pixel frame, and no test would have said so.

It also reads the stylesheet the shell carries and fails if a rule is not scoped to `.tb-viz`, if
the tray is not hidden until it is hovered, or if `.tb-viz[hidden]` goes missing — the line a host
depends on to hide the player while it fetches a replay. And it imports the built shell, because a
backtick inside a CSS comment ends the template literal and `node --check` on a `.js` file reads the
wreckage as a script and says nothing.

The bundle is not committed. It lands in `../dist/viz/` beside the component it was transpiled
from, and ships in the artifact image under `/artifacts/viz/`, so only someone changing the viewer
needs Node. `engine.json` records the component digest the bundle carries: a replay naming a
different `engine_digest` is one this build cannot faithfully show, and `tinybrains view` says so
rather than drawing it anyway.

## Layout

```text
src/engine.js   the cartridge in the browser -- decode one frame, or a range in one pass
src/render.js   pixels: terrain, territory, rings, food, hills, ants, and the zoom/pan geometry. Decides nothing
src/shell.js    the player: title bar, transport, timeline with event marks, the tools tray, the
                stylesheet, and what is drawn over the board -- the rings' step counts and the
                territory's fold
src/map.js      the map visual: a board on its own at turn zero, its name, player count and size
check.mjs       checks that need no browser: geometry, step counts, territory, labels, CSS scoping
src/index.js    mount() and mountMap() -- built to viz.js, the path the platform loads
src/react.js    the same viewer, and the map visual, as React components; React is a peer
```

## Three choices worth knowing

**Territory comes from the engine; the rings do not need it.** Vision is a rule — a radius, on a
board that wraps — and a viewer that drew it would be a second engine free to disagree with the
first. So each `replay-decode` frame says what each seat saw for the first time that turn, out of
the same `known` mask its observations are built from, and the shell only folds those from turn
zero. How many moves an ant is from a hill is the other case: it decides nothing about the match, so
the shell counts them itself — round water, since a ring that lit up for an enemy on the far side of
a wall would point at nothing, and ignoring food, which comes and goes and would make a ring flicker.

**The shell lives here, not in the web application.** The first contract put it in the platform
and left the cartridge "owning pixels". The viewer has three consumers that are not one
application, and a shell split across three of them is a shell maintained in three places. The
trade is that a second cartridge writes its own scrubber, and the extraction point is whenever that
cartridge exists and there are two real viewers to generalise from.

**It is framework-free, with a React wrapper.** This is a canvas, a slider and a few readouts;
React buys it nothing and would tie the cartridge to a React version. `react.js` is the only file
that mentions React at all.

## What the viewer never does

**Run a model.** It re-simulates from recorded actions — no ONNX, no adapter, no competitor code
in the browser. That is why the docs and the marketing site can embed it without inheriting the
evaluator's security surface.
