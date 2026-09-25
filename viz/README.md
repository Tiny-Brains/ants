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
<AntsReplay replay={envelope} tier="player" onTurn={(f) => setScore(f.score)} />
```

The web application imports `viz.js` directly and serves only the modules it needs, not `react.js`.

`mount(target, replay, opts)` takes an element or a selector, and the envelope or a URL to fetch it
from. A replay carries its own board, so the envelope is the only input. `viz.js` also exports
`drawFrame`, `mountGraph`, `mountMap`, `optsFromHash`, `Viewer`, `SEATS`, `frameAt`, `allFrames`,
`board` and `meta`.

## Tiers

`opts.tier` picks what the viewer draws, by the job it does on the page. It defaults to `stage`, so
the book and `tinybrains view` get what they always had.

| Tier | Draws | Controls |
|---|---|---|
| `stage` | seat cards with name, owner, ants alive, hills standing and razed, score; the board with its tray; the transport | keys, zoom, territory, speed, the key list, fullscreen |
| `player` | seat cards with name, owner and score; the board; first, previous, play, next, last, the timeline, the turn | fullscreen, which promotes it to a stage in place (same element, canvas and frames) and back when it ends |
| `tile` | the board in a 16:10 box, letterboxed on the void; names and scores laid over it (two seats in mirrored top corners, three to eight the two leaders and "+N"); the turn bottom right | none: no focus, no keys, no pointer. `preview()` and `stop()` |
| `thumb` | the board in whatever box the host gives it (16:10 unless it sets a height); a score chip from 160 pixels wide | none |

**Seat cards.** One a seat: the seat's colour as its left edge, name and owner on top, the score
large at the right. On the stage a second line shows the ants alive and one square a hill, filled
while it stands and hollow once razed (the razed ones worked out from the envelope's `map.hills`,
since a frame lists only the hills still standing). `seatColumns()` picks the columns from the seat
count and the width alone, at most four a row, so five to eight seats wrap onto two rows and
nothing a turn changes can move the board. Below 640 pixels (the viewer's width, not the window's)
the cards go two to a row, the owners drop and the timeline takes a row of its own.

**A tile or a thumb needs no replay.** Mount it with `replay: null` and one of:

- `frame`: a match's last frame, as `GET /v1/matches/{id}/frame` serves it in `.frame`. It carries
  `size` and `water`, so it draws without the board file and without calling the component.
- `board`: the map file, for a match nobody has played yet. Turn zero comes from the cartridge
  (`replay-decode` over an envelope of no moves, as the map visual does), with no scores and no turn.

A tile with no envelope needs `labels`: nothing else names its seats. A tile or a thumb given both
a `frame` and a replay rests on the frame. Neither ever plays into another match: a replay ends on
its last frame and stays there, in every tier.

```js
import { mount, drawFrame } from "/cartridges/ants/viz.js";
const { frame, seats } = await (await fetch(`/v1/matches/${id}/frame`)).json();
const labels = seats.map((s) => ({ seat: s.seat, name: s.name, by: s.by }));

const tile = await mount(card, null, { tier: "tile", frame, labels });   // or { board } when frame is null
card.onmouseenter = () => setTimeout(() => tile.preview(replayUrl), 600); // the host owns the delay
card.onmouseleave = () => tile.stop();

drawFrame(row, frame);   // a thumb, synchronously; { tier: "tile", labels } for a tile
```

`drawFrame(target, frame, opts)` is `mount` for a stored frame: synchronous, a thumb unless
`opts.tier` is `"tile"`, and it never calls the component. It adapts the frame's `size: [rows,
cols]` and `water: { rle }` into the board the renderer reads.

`tile.preview(replay | url)` fetches the replay once and keeps it, decodes only the last forty
turns with `replay-decode`'s range form (`[max(0, turns − 40), turns]`), plays them once and rests
on the last. `tile.stop()` goes back to the frame the tile rests on. The host keeps one preview
playing at a time.

## The player

The stage and the player are two bars and a board: the seat cards above it and the transport
below it. The zoom buttons and the territory toggle sit in a tray over the board's top-right corner,
hidden until hovered, focused or touched; the key list opens over its bottom-left corner.

| Control | Does |
|---|---|
| Transport | first, previous, play/pause, next, last |
| Timeline | click to jump, drag to scrub; ticks mark a hill razed or a colony wiped out, in the seat's colour |
| Zoom | stage only. Wheel about the cursor, drag to pan, −/+/fit buttons; opens fitted and stays fitted through a resize until zoomed |
| Hill rings | a hill with an enemy ant within eight moves (round water, across the wrap) is ringed in its owner's colour |
| Territory | stage only. The eye button or `E`: fog where nobody has looked, each seat's explored area and frontier, its share on its card |
| Speed | stage only. Cycles 0.25× to 8× |
| Fullscreen | the button or `F`. Where the browser has no fullscreen for an element (a phone), the viewer fills the window instead |
| Keys | `space` play/pause · `←` `→` step (shift for ten) · `↑` `↓` ten · `Home` `End` · `F` fullscreen; on a stage also `+` `−` `0` zoom · `E` territory · `?` the key list · `Esc` closes it |

The root has `contain: inline-size`, so the viewer takes the width its host gives it. A host that
sizes to its content (an inline-block, a float) must give it a width.

## The public surface

What a host, and `graph.js`, may read and call on a `Viewer`. Everything else on it is internal.

| Member | Is |
|---|---|
| `series()` | `{ ants, hills, score }`, each `[seat][turn]`: ants alive, hills standing and score, counted once from the frames |
| `events` | `[{ i, turn, seat, kind, what }]`: `kind` is `"razed"` (a hill of `seat`'s fell) or `"wiped"` (its last ant died) |
| `range` | `{ lo, hi }`: the turns the timeline covers |
| `turn` | the turn shown |
| `seek(t)` | show turn `t`, clamped to the range |
| `on("turn" \| "layout", fn)` | `"turn"` calls `fn(turn, frame)` as each is shown; `"layout"` calls `fn(trackBox())` when the timeline may have moved. Returns the unsubscribe |
| `trackBox()` | the timeline track's `{ left, width }` in page pixels, or null in a tier without one |
| `labels` | `[{ name, by }]` a seat, as shown |
| `tier` | the tier drawn now (a promoted player reads `"stage"` while it is fullscreen) |
| `preview(replay \| url)`, `stop()` | a tile's |
| `playing`, `play()`, `pause()` | whether it is playing, and the transport's play and pause |
| `fullscreen(on?)` | a stage's or a player's; toggles without an argument |
| `destroy()` | take it off the page |

In a replay a frame's index and its turn are the same number, so `series()` is indexed by turn.
`onTurn` stays the host's single callback.

## The ants graph

Each seat's ants alive, hills standing or score over the match, under a stage or a player and the
same width as it: one thin line a seat in its colour behind a three-way switch (ants is the
default), a playhead that follows the viewer, and the razed and wiped-out ticks on the line of the
seat that lost them. Hovering scrubs the viewer, pausing it while the pointer is over the graph.
A legend row sits underneath.

```js
import { mount, mountGraph } from "/cartridges/ants/viz.js";
const viewer = await mount("#replay", envelope, { tier: "stage" });
const graph = await mountGraph("#graph", viewer, { kind: "ants" });   // kind, height (140), theme
// graph.destroy() before viewer.destroy()
```

Its x-axis is the timeline's: it reads `trackBox()` and redraws on `"layout"`, so a tick on the
graph sits under the same tick on the timeline. Mounted where the track is out of its reach, it
keeps gutters of its own. Hills and score are step series, and two seats level on a value would
draw one line, so each seat's step line is moved a fixed `(seat − (n − 1) / 2) × 1.5` pixels; the
values stay exact, and ants alive, a curve, is not moved. `graph.js` is loaded by `mountGraph` on
first use, not with `viz.js`.

## Options and hash parameters

| Option | Meaning |
|---|---|
| `tier` | `"stage"` (default), `"player"`, `"tile"` or `"thumb"`; see §Tiers |
| `frame` | A tile's or a thumb's stored frame, drawn with no replay |
| `board` | A tile's or a thumb's map file, drawn at turn zero with no replay |
| `turn` | The turn to open on |
| `from`, `to` | Narrow the timeline without renumbering it: a tutorial shows turns 40–60 and the reader still sees "turn 47" |
| `autoplay`, `speed` | Start playing, and how fast |
| `zoom`, `centre` | Open zoomed about `[row, col]` |
| `theme` | `"light"` or `"dark"`; overrides the page |
| `chrome` | `"hover"` (default) or `"always"`, which pins the tray open |
| `height` | The whole player's height |
| `stageHeight` | The board's height; the player is that plus its bars. Wins over `height` |
| `labels` | `[{ seat, name, by }]`: what the host calls each seat. Without it a seat shows the envelope's name; a tile with no envelope requires it |
| `explored` | Open with territory drawn |
| `onTurn` | Called with each frame shown |

`height`, `stageHeight`, `zoom`, `centre`, `chrome` and `explored` are a stage's or a player's (the
last four a stage's); a tile and a thumb are boxes whose shape the host sets. `AntsReplay` takes
`replay` and the same options except `height`, `zoom`, `centre`, `frame` and `board`, plus `style`
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
`engine/src/tests/fixtures/replay-maze-03.json` through the geometry. Over a small fake document it
checks the parts each tier builds; the seat cards at three to eight seats, wrapping onto two rows
from five and going two a row with no owners below 640 pixels; the tile's overlay at two and eight
seats; `drawFrame` drawing a stored eight-seat last frame through a copy of the viewer whose
component throws on any call; the preview's range at its ends (under forty turns, and turn zero);
`series()` against the frames' own counts; the public surface; fullscreen promoting a player
without a remount; and the graph lining its x-axis up with the timeline. It fails if a stylesheet
rule is not scoped to `.tb-viz`, if the tray is not hidden until hovered, or if `.tb-viz[hidden]` is
missing (a host hides the player with it while it fetches a replay). It imports the built shell,
because a backtick inside a CSS comment ends the template literal and `node --check` does not
notice.

The stored frame is `last-frame-basic-xlarge-8p.json`, the last frame of a thousand-turn match on
the envelope's top board with eight seats (about 7 KB), made by `make-last-frame.mjs` and never by
hand. It plays the match with the `tinybrains` CLI and the starter's models against a registry
pointing at `../dist`, then decodes its last turn through the component, as the runner does:

```sh
STARTER_MODELS=../../ants-starter/models node make-last-frame.mjs   # a minute or two
node make-last-frame.mjs --replay some-replay.json                 # or from a replay you have
```

`engine.json` records the component digest the bundle was transpiled from. A replay naming a
different `engine_digest` is one this build cannot faithfully show, and `tinybrains view` says so.

## Layout

```text
src/index.js    mount(), drawFrame(), mountGraph(), mountMap(), optsFromHash() -- built to viz.js,
                the path the platform loads
src/shell.js    the viewer: the tiers, the seat cards, transport, timeline and its marks, the tray,
                the tile's overlay, the public surface, the stylesheet, and what is drawn over the
                board (ring step counts, the territory's fold)
src/render.js   pixels: terrain, territory, rings, food, hills, ants, and the zoom/pan geometry
src/engine.js   the cartridge in the browser: decode one frame, a range, or a board's turn zero
src/map.js      the map visual: a board on its own at turn zero
src/graph.js    the ants graph: ants, hills and score per seat over the match
src/react.js    AntsReplay and AntsMap, the same viewer as React components (web does not use them)
check.mjs       checks that need no browser
make-last-frame.mjs              makes the stored frame check.mjs draws
last-frame-basic-xlarge-8p.json  that frame
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
  copy `viz.js shell.js render.js engine.js map.js graph.js` and the transpiled component by name.
  A new static import from `viz.js` is a change to both lists; that is why `map.js` and `graph.js`
  load on first use, so a host whose list lacks one loses that visual and keeps every replay.
- **Nothing plays into another match.** A replay ends on its last frame in every tier, and a
  tile's preview rests on its last; choosing the next match is the host's page, never the viewer.
- **A tile or a thumb given a frame never calls the component.** `check.mjs` proves it with the
  component stubbed to throw.
- **The bundle and the component travel together.** Neither is committed; both land in `dist/` and
  ship in one release, and `engine.json` names the digest the bundle carries.
- **`react.js` is the only file that mentions React.**
