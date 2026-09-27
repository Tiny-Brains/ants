// The player: everything around the pixels.
//
// It behaves like a media player because that is what watching a match is — transport buttons, a
// timeline you can click and drag, playback speed, and a keyboard that does what a keyboard does in
// a player. One thing it does that a video player cannot: the timeline is annotated, so a hill razed
// or a colony wiped out can be found without scrubbing for it. And three things are drawn over the
// board to say what happened and where to look: the turn's deaths with a line from every killer,
// the hills that fell, and a ring around any hill an enemy is within eight moves of -- plus, on a
// toggle, the territory each seat has explored. The first two come from the frame's own record; no
// rule is re-read.
//
// **Four tiers, one viewer.** A host picks the tier by the job the viewer does on its page, and the
// tier decides which parts `build()` makes; it never changes what is simulated.
//
//   stage    seat cards with ants and hills, the board with its tray, the full transport with speed,
//            the key list and fullscreen. The default, so the book and `tinybrains view` are this.
//   player   seat cards with name, owner and score, the board, the transport and fullscreen -- no
//            tray, no zoom, no territory. Fullscreen promotes it to a stage in place, no remount.
//   tile     the board in a 16:10 box with the seats' names and scores laid over it and the turn in
//            a corner. Nothing to operate: `preview()` plays the last forty turns, `stop()` rests it.
//   thumb    one frame of the board, and a score chip once the box is 160 pixels wide.
//
// A tile or a thumb can be drawn with no replay at all: from the last frame Soma keeps for a match
// (`frame`), which carries its own size and water, or from the board alone at turn zero (`board`).
// Neither decodes anything, which is what lets a grid of cards draw at the speed of a grid.
//
// Three rules this file keeps.
//
// **Two bars and a board**, on the stage and the player. The seats are cards above the board and
// the transport a bar below it, and both are always on screen: who is playing and what the score is
// are read the whole way through a match, so they are not something to go and find. The tools --
// zoom and the territory toggle -- are the only layer over the stage, and appear on hover, on
// keyboard focus and on a touch, the way a video player's chrome does. `chrome: "always"` pins them
// open. A tile's names are a layer too, but a DOM one outside the drawing, so a long name never
// moves a cell.
//
// **The chrome follows the page; the board does not.** Every colour of the frame is one of the
// platform's design tokens with a written-out fallback, so the player is the colour of the card it
// sits in and follows a theme switch with no work here. The board keeps its own fixed palette in
// both themes: a match has to look like itself, the way a video does not change colour with the
// player around it.
//
// **Every rule in the stylesheet starts at `.tb-viz`.** One `<style>` goes into the host document
// on mount; it is not a shadow root, and `check.mjs` fails the build on an unscoped rule. An
// unscoped `.tb-bar` in here once landed on the web application's own header.

import { allFrames, framesBetween, openingOf } from "./engine.js";
import { Renderer, SEATS, expandRle } from "./render.js";

const CSS = `
.tb-viz{
  /* Roles, not literals: var(--ink, …) is the platform's token where the host loads
     design-system/tokens.css, and the written-out Cobalt value everywhere else. */
  --tb-ink:var(--ink,#142642);
  --tb-dim:var(--muted,#536780);
  --tb-line:var(--line,#B9C9E1);
  --tb-panel:var(--surface,#FFFFFF);
  --tb-raised:var(--surface-raised,#EAF0FC);
  --tb-accent:var(--accent,#255FC5);
  --tb-accent-ink:var(--accent-ink,#FFFFFF);
  --tb-bad:var(--danger,#BF354D);
  --tb-sans:var(--font-sans,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif);
  --tb-mono:var(--font-mono,ui-monospace,"SF Mono",Menlo,Consolas,monospace);
  --tb-r:var(--radius-sm,6px);
  /* The board's own ground, and the tray that is read against it. Fixed in both themes. */
  --tb-void:#05080F;
  --tb-over:#0D1729;
  --tb-over-line:rgba(238,243,255,.16);
  --tb-over-ink:#EEF3FF;
  --tb-over-dim:#A6B5D1;

  color:var(--tb-ink);background:var(--tb-panel);
  display:flex;flex-direction:column;min-height:0;
  font:13px/1.5 var(--tb-sans);
  overflow:hidden;-webkit-font-smoothing:antialiased;
  contain:inline-size}
/* Its width is the host's, never its content's. The seat cards are lines of text and the canvas
   is drawn at the pixels it was last given, and either would otherwise become the viewer's minimum
   width: a grid column holding it grew to fit, which put the web application's home-page replay at
   871 pixels in a 410-pixel column. A host that sizes to its content has to give it a width. */
/* No border and no radius of its own. mount() styles the host element rather than making a root
   inside it, so a frame the host has already drawn -- the web application's card, the book's figure
   -- is the frame, and a second one drawn here would sit inside it. Clipping is this side's: the
   stage paints to the corners and something has to cut them. */

/* The host page's dark palette, for a host that ships no tokens of its own. Where it does ship
   them the var() above already answered and every line here resolves to the same value twice. */
@media (prefers-color-scheme:dark){.tb-viz:not([data-tb-theme=light]){
  --tb-ink:var(--ink,#EEF3FF);--tb-dim:var(--muted,#A6B5D1);--tb-line:var(--line,#344764);
  --tb-panel:var(--surface,#121D32);--tb-raised:var(--surface-raised,#1C2B46);
  --tb-accent:var(--accent,#86B2FF);--tb-accent-ink:var(--accent-ink,#0B224A);
  --tb-bad:var(--danger,#FF9A9A)}}

/* An asked-for theme is authoritative: literals, so theme:"dark" is dark inside a light page. */
.tb-viz[data-tb-theme=light]{
  --tb-ink:#142642;--tb-dim:#536780;--tb-line:#B9C9E1;--tb-panel:#FFFFFF;--tb-raised:#EAF0FC;
  --tb-accent:#255FC5;--tb-accent-ink:#FFFFFF;--tb-bad:#BF354D}
.tb-viz[data-tb-theme=dark]{
  --tb-ink:#EEF3FF;--tb-dim:#A6B5D1;--tb-line:#344764;--tb-panel:#121D32;--tb-raised:#1C2B46;
  --tb-accent:#86B2FF;--tb-accent-ink:#0B224A;--tb-bad:#FF9A9A}

.tb-viz *{box-sizing:border-box}
/* The root sets display:flex on the host element, which outranks the [hidden] attribute's UA rule
   unless it is said again here -- and a frame that hides the viewer while it loads has to work. */
.tb-viz[hidden]{display:none}
.tb-viz:focus{outline:none}
.tb-viz:focus-visible{outline:2px solid var(--tb-accent);outline-offset:-2px}

/* ---------- the seat cards: every seat, always on screen ----------
   One card per seat, so each number sits with the player it belongs to: the seat's colour as the
   card's left edge, name and owner on top, the score large at the right, and on the stage a second
   line with the ants alive and one square a hill -- filled while it stands, hollow once it is
   razed. The flat row this replaced ran four seats' ants, hills and scores into one line, and the
   eye had to count along it to find whose 12 that was.

   A grid, because four seats and eight do not fit one line of the frames the player is given.
   seatColumns() picks the columns from the seat count and the width and nothing else, so how many
   rows there are never depends on the text in them: a count that grows by a digit, or a long name,
   cannot move the board under it. Every card of a tier is the same height for the same reason. */
.tb-viz .tb-top{display:grid;grid-template-columns:repeat(var(--tb-cols,2),minmax(0,1fr));gap:6px;
  flex:none;min-width:0;padding:8px 10px;background:var(--tb-panel);
  border-bottom:1px solid var(--tb-line)}
.tb-viz .tb-seat{display:grid;grid-template-columns:minmax(0,1fr) auto;column-gap:10px;row-gap:1px;
  align-items:center;min-width:0;padding:5px 10px 5px 9px;border-radius:var(--tb-r);
  border-left:3px solid var(--tb-seat);
  background:color-mix(in srgb,var(--tb-seat) 9%,var(--tb-raised))}
.tb-viz .tb-seat[data-out=true]{opacity:.55}
/* The name and the owner share the top line, on a line that wraps but is one line tall. An owner
   with no room for seven characters beside the name wraps onto the line nobody sees, rather than
   standing there cut to a lone "@"; a name too long for the line is cut on its own. */
.tb-viz .tb-id{grid-column:1;grid-row:1;display:flex;flex-wrap:wrap;align-items:baseline;
  column-gap:6px;min-width:0;height:1.5em;overflow:hidden;white-space:nowrap}
.tb-viz .tb-name{font-weight:650;flex:0 1 auto;min-width:0;overflow:hidden;text-overflow:ellipsis}
.tb-viz .tb-by{color:var(--tb-dim);font-size:12px;flex:1 1 7ch;max-width:max-content;min-width:0;
  overflow:hidden;text-overflow:ellipsis}
/* The score at the right, where a scoreboard keeps it, the one figure on the card in large type.
   It spans both lines of a stage card, so the card's height is the two lines and not the digits. */
.tb-viz .tb-score{grid-column:2;grid-row:1 / span 2;font:650 22px/1 var(--tb-mono);
  font-variant-numeric:tabular-nums;letter-spacing:-.01em}
.tb-viz .tb-nums{grid-column:1;grid-row:2;display:flex;align-items:center;gap:10px;min-width:0;
  height:15px;font:11px/1 var(--tb-mono);font-variant-numeric:tabular-nums;color:var(--tb-dim)}
.tb-viz .tb-n{display:inline-flex;align-items:center;gap:4px}
.tb-viz .tb-n[hidden]{display:none}
.tb-viz .tb-n b{font-weight:600;color:var(--tb-ink)}
/* Room held for the digits a count reaches, so ants passing ten do not shift the hills. */
.tb-viz .tb-n-ants b{min-width:2ch}
.tb-viz .tb-n-seen b{min-width:3ch}
.tb-viz .tb-ant{width:7px;height:7px;border-radius:50%;background:var(--tb-seat);flex:none}
.tb-viz .tb-hills{gap:3px;color:var(--tb-seat)}
.tb-viz .tb-hill{width:8px;height:8px;border:1.5px solid currentColor;border-radius:2px;flex:none}
.tb-viz .tb-hill[data-on]{background:currentColor}
.tb-viz .tb-eye svg{display:block}
/* The player keeps name, owner and score: one line, a smaller score, no counts. */
.tb-viz.tb-tier-player .tb-nums{display:none}
.tb-viz.tb-tier-player .tb-score{grid-row:1;font-size:18px}
/* A narrow viewer -- under 640 pixels, which seatColumns() also reads -- sets its cards two to a
   row and drops the owners before it cuts a name. */
.tb-viz[data-tb-narrow] .tb-by{display:none}
.tb-viz[data-tb-narrow] .tb-top{gap:5px;padding:6px 8px}
.tb-viz[data-tb-narrow] .tb-score{font-size:18px}

/* ---------- the stage ---------- */
.tb-viz .tb-stage{position:relative;flex:1 1 auto;min-height:200px;display:flex;overflow:hidden;
  background:var(--tb-void);cursor:grab}
.tb-viz .tb-stage.tb-drag{cursor:grabbing}
.tb-viz .tb-stage canvas{display:block;margin:auto}
/* Only a stage zooms and pans; everywhere else the board is looked at, not held. */
.tb-viz:not(.tb-tier-stage) .tb-stage{cursor:default}

/* ---------- the tray ----------
   One layer over the board, out of the way until it is wanted. Flat and opaque rather than a
   blurred pane: the pane read as terrain over a busy board, which is the one place it must not. */
.tb-viz .tb-tray{position:absolute;inset:0;display:flex;flex-direction:column;
  justify-content:space-between;gap:8px;padding:9px;pointer-events:none;
  opacity:0;transition:opacity .16s ease}
.tb-viz .tb-row{display:flex;align-items:flex-start;gap:8px;min-width:0}
.tb-viz .tb-head{transform:translateY(-5px);transition:transform .16s ease}
.tb-viz:is(:hover,:focus-within,[data-tb-peek],[data-tb-chrome=always]) .tb-tray{opacity:1}
.tb-viz:is(:hover,:focus-within,[data-tb-peek],[data-tb-chrome=always]) .tb-head{transform:none}
.tb-viz .tb-pane{background:var(--tb-over);border:1px solid var(--tb-over-line);
  border-radius:var(--tb-r);color:var(--tb-over-ink)}

/* The only part of the tray that is clickable, so a drag anywhere else still pans the board. */
.tb-viz .tb-tools{display:flex;gap:2px;margin-left:auto;padding:3px;pointer-events:auto;flex:none}
.tb-viz .tb-tools button{width:28px;height:28px;padding:0;border-radius:6px;
  color:var(--tb-over-ink)}
.tb-viz .tb-tools button:hover:not(:disabled){background:rgba(238,243,255,.14)}
.tb-viz .tb-tools button[aria-pressed=true]{background:rgba(238,243,255,.24)}
.tb-viz .tb-tools .tb-sep{width:1px;margin:5px 3px;background:var(--tb-over-line)}

/* The key list, over the board's bottom-left corner while its button is pressed. */
.tb-viz .tb-keys{position:absolute;left:9px;bottom:9px;margin:0;padding:8px 12px;
  display:grid;grid-template-columns:auto auto;gap:3px 14px;font-size:12px;line-height:1.4}
.tb-viz .tb-keys[hidden]{display:none}
.tb-viz .tb-keys dt{font:600 11.5px/1.4 var(--tb-mono);white-space:nowrap}
.tb-viz .tb-keys dd{margin:0;color:var(--tb-over-dim)}

/* Parts only a stage has. A player that was promoted to a stage for fullscreen keeps them built
   when it comes back, and this is what puts them away. */
.tb-viz.tb-tier-player .tb-so{display:none}

/* ---------- the transport, which is always on screen ---------- */
.tb-viz .tb-bar{display:flex;flex-wrap:wrap;align-items:center;gap:6px 10px;padding:8px 10px;
  flex:none;background:var(--tb-panel);border-top:1px solid var(--tb-line)}
.tb-viz button{font:inherit;color:var(--tb-ink);background:transparent;border:1px solid transparent;
  border-radius:6px;padding:5px 7px;cursor:pointer;line-height:1;display:inline-flex;
  align-items:center;justify-content:center}
.tb-viz button:hover:not(:disabled){background:var(--tb-raised)}
.tb-viz button:disabled{opacity:.35;cursor:default}
.tb-viz button:focus-visible{outline:2px solid var(--tb-accent);outline-offset:2px}
.tb-viz .tb-transport{display:flex;gap:2px;align-items:center}
.tb-viz .tb-transport button{width:32px;height:32px;padding:0}
.tb-viz .tb-play{background:var(--tb-accent);color:var(--tb-accent-ink);
  width:36px;height:36px;border-radius:50%}
.tb-viz .tb-play:hover:not(:disabled){background:var(--tb-accent);filter:brightness(1.06)}
.tb-viz .tb-end{display:flex;align-items:center;gap:2px;flex:none}
.tb-viz .tb-end button{height:30px;min-width:30px;padding:0 5px}
.tb-viz .tb-end button[aria-pressed=true]{background:var(--tb-raised)}

.tb-viz .tb-track{position:relative;flex:1 1 80px;height:28px;display:flex;align-items:center;
  cursor:pointer;touch-action:none;min-width:80px}
.tb-viz .tb-track:focus-visible{outline:2px solid var(--tb-accent);outline-offset:2px;border-radius:6px}
.tb-viz .tb-rail{position:absolute;left:0;right:0;height:5px;border-radius:3px;
  background:var(--tb-raised)}
.tb-viz .tb-fill{position:absolute;left:0;height:5px;border-radius:3px;background:var(--tb-accent)}
.tb-viz .tb-mark{position:absolute;width:2px;height:12px;border-radius:1px;
  transform:translateX(-1px);opacity:.9}
.tb-viz .tb-mark[data-kind=wiped]{width:3px;height:16px}
.tb-viz .tb-thumb{position:absolute;width:13px;height:13px;border-radius:50%;
  background:var(--tb-accent);border:2px solid var(--tb-panel);transform:translateX(-6.5px);
  box-shadow:0 1px 3px rgba(0,0,0,.3)}
.tb-viz .tb-turn{font:12px/1 var(--tb-mono);font-variant-numeric:tabular-nums;color:var(--tb-dim);
  white-space:nowrap;min-width:74px;text-align:right}
.tb-viz .tb-speed{font:11px/1 var(--tb-mono);min-width:36px;color:var(--tb-dim)}
.tb-viz .tb-err{padding:16px;color:var(--tb-bad);font:13px/1.6 var(--tb-sans)}
/* A phone: the timeline takes a row of its own above the buttons, and speed and the key list go. */
.tb-viz[data-tb-narrow] .tb-track{order:-1;flex:1 1 100%}
.tb-viz[data-tb-narrow] .tb-transport{margin-right:auto}
.tb-viz[data-tb-narrow] :is(.tb-speed,.tb-keys-btn){display:none}

/* ---------- fullscreen ----------
   The stage with the page gone. The board takes whatever the seats and the transport leave, over
   any height the host gave it. Where the browser has no fullscreen for an element -- a phone's --
   the viewer fills the window instead, and that is the same rule. */
.tb-viz:fullscreen,.tb-viz[data-tb-full]{width:100%;height:100% !important}
.tb-viz[data-tb-full]{position:fixed;inset:0;z-index:2147483000}
.tb-viz:fullscreen .tb-stage,.tb-viz[data-tb-full] .tb-stage{flex:1 1 auto !important;
  height:auto !important}

/* ---------- the tile and the thumb: a board in a box ----------
   The box is 16:10 unless the host gives it a height, and the board keeps its own shape inside it,
   letterboxed on the void. Nothing here takes focus, a key or a pointer. */
.tb-viz:is(.tb-tier-tile,.tb-tier-thumb){position:relative;aspect-ratio:16 / 10}
.tb-viz:is(.tb-tier-tile,.tb-tier-thumb) .tb-stage{position:absolute;inset:0;min-height:0}
/* The tile's overlay: a DOM layer over the canvas, never drawn into it, in the tray's fixed colours
   because it is read against the board and not against the page. */
.tb-viz .tb-over{position:absolute;inset:0;pointer-events:none;color:var(--tb-over-ink);
  font:600 12px/1.3 var(--tb-sans)}
.tb-viz .tb-tag{display:flex;align-items:center;gap:5px;min-width:0;white-space:nowrap}
.tb-viz .tb-tag i{width:8px;height:8px;border-radius:2px;flex:none;background:var(--tb-seat)}
.tb-viz .tb-tag .tb-nm{min-width:0;overflow:hidden;text-overflow:ellipsis}
.tb-viz .tb-tag .tb-sc{font:650 13px/1 var(--tb-mono);font-variant-numeric:tabular-nums}
.tb-viz .tb-corner{position:absolute;top:6px;max-width:calc(50% - 9px);padding:3px 7px;
  border-radius:6px;background:rgba(13,23,41,.82)}
.tb-viz .tb-corner.tb-l{left:6px}
.tb-viz .tb-corner.tb-r{right:6px;flex-direction:row-reverse}
.tb-viz .tb-list{position:absolute;top:6px;left:6px;display:grid;gap:2px;max-width:calc(100% - 12px);
  padding:4px 8px;border-radius:6px;background:rgba(13,23,41,.82)}
.tb-viz .tb-list .tb-sc{margin-left:auto;padding-left:10px}
.tb-viz .tb-more{color:var(--tb-over-dim);font-weight:500;font-size:11.5px}
.tb-viz .tb-at{position:absolute;right:6px;bottom:6px;padding:2px 6px;border-radius:6px;
  background:rgba(13,23,41,.82);font:600 11.5px/1.2 var(--tb-mono);
  font-variant-numeric:tabular-nums}
.tb-viz .tb-at[hidden]{display:none}
/* The thumb's one addition, from 160 pixels up. */
.tb-viz .tb-chips{position:absolute;left:5px;bottom:5px;display:flex;align-items:center;gap:6px;
  padding:2px 6px;border-radius:6px;background:rgba(13,23,41,.82);color:var(--tb-over-ink);
  font:600 11.5px/1.2 var(--tb-mono);font-variant-numeric:tabular-nums;pointer-events:none}
.tb-viz .tb-chips[hidden]{display:none}
.tb-viz .tb-chips i{display:inline-block;width:7px;height:7px;border-radius:2px;margin-right:4px;
  background:var(--tb-seat)}
.tb-viz .tb-chips .tb-chip{display:inline-flex;align-items:center;min-width:0}
.tb-viz .tb-chips .tb-nm{max-width:56px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;margin-right:5px}

/* ---------- the map visual: a board, not a match (map.js) ----------
   Three facts over the board and nothing else: its name, how many play it, how many cells it is.
   No seats, no transport, no tray, no hover -- a board page is read, not operated. The name gives
   way to an ellipsis before either number does, because the numbers are what a list of boards is
   scanned by. The board keeps the match palette, on the same void. */
.tb-viz[data-tb-mode=map]{cursor:default}
.tb-viz .tb-map-head{display:flex;align-items:center;gap:10px;padding:7px 10px;flex:none;
  min-width:0;background:var(--tb-panel);border-bottom:1px solid var(--tb-line)}
.tb-viz .tb-map-name{font:600 13px/1.4 var(--tb-mono);flex:1 1 auto;min-width:0;overflow:hidden;
  text-overflow:ellipsis;white-space:nowrap}
.tb-viz .tb-map-fact{display:inline-flex;align-items:center;gap:4px;flex:none;
  font:12px/1 var(--tb-mono);font-variant-numeric:tabular-nums;color:var(--tb-dim)}
.tb-viz .tb-map-fact svg{display:block}
.tb-viz .tb-map-board{display:flex;flex:none;overflow:hidden;background:var(--tb-void)}
.tb-viz .tb-map-board canvas{display:block;margin:auto}

/* ---------- the ants graph (graph.js) ----------
   The match as three lines a seat, under the player and the same width as it: a switch, the plot,
   and a legend for a reader who has scrolled the seat cards away. The plot's colours are the
   seats'; its grid and playhead are the page's. */
.tb-viz[data-tb-mode=graph]{cursor:default}
.tb-viz .tb-g-head{display:flex;align-items:center;gap:10px;padding:6px 10px;flex:none;min-width:0;
  border-bottom:1px solid var(--tb-line)}
.tb-viz .tb-seg{display:inline-flex;gap:2px;padding:2px;flex:none;border-radius:6px;
  background:var(--tb-raised)}
.tb-viz .tb-seg button{padding:4px 10px;font-size:12px;border-radius:4px;color:var(--tb-dim)}
.tb-viz .tb-seg button[aria-pressed=true]{background:var(--tb-panel);color:var(--tb-ink);
  box-shadow:0 0 0 1px var(--tb-line)}
.tb-viz .tb-g-lab{margin-left:auto;min-width:0;overflow:hidden;text-overflow:ellipsis;
  white-space:nowrap;font:12px/1.3 var(--tb-mono);font-variant-numeric:tabular-nums}
.tb-viz .tb-g-plot{position:relative;flex:none;min-width:0}
.tb-viz .tb-g-plot canvas{display:block;cursor:crosshair;touch-action:none}
.tb-viz .tb-legend{display:flex;flex-wrap:wrap;gap:4px 14px;padding:4px 10px 8px;flex:none;
  font-size:12px;color:var(--tb-dim)}
.tb-viz .tb-legend span{display:inline-flex;align-items:center;gap:6px;min-width:0}
.tb-viz .tb-legend i{width:12px;height:3px;border-radius:2px;flex:none;background:var(--tb-seat)}
.tb-viz .tb-legend b{color:var(--tb-ink);font-weight:600}

@media (prefers-reduced-motion:reduce){
  .tb-viz .tb-tray,.tb-viz .tb-head{transition:none}}
`;

const STYLE_ID = "tb-viz-style";

/**
 * One stylesheet per document, not one per process.
 *
 * A module-level "done" flag is the obvious shape and the wrong one: a second document — an
 * iframe, a print window, a test harness — would then get a viewer with no stylesheet at all,
 * because the first document had already claimed the flag.
 */
export function injectCss(doc) {
  if (doc.getElementById(STYLE_ID)) return;
  const el = doc.createElement("style");
  el.id = STYLE_ID;
  el.textContent = CSS;
  doc.head.appendChild(el);
}

/** The tiers, in the order of how much each one draws. */
export const TIERS = ["stage", "player", "tile", "thumb"];
/** The tiers with seat cards and a transport: the ones that play a match rather than show one. */
const FULL = new Set(["stage", "player"]);

const SPEEDS = [0.25, 0.5, 1, 2, 4, 8];
const TURNS_PER_SECOND = 10;
/** How long the tray stays up after a touch, which has no hover to keep it up. */
const PEEK_MS = 2600;
/** How near an enemy ant has to be to a hill, in moves, before the hill is ringed. */
const THREAT_STEPS = 8;
/** The narrowest a seat card may be: its edge, seven or eight letters of its name and its score.
 *  Six seats in an 800-pixel frame are then two rows of three. */
const MIN_SEAT = 160;
/** The most cards a row holds. Five to eight seats wrap onto two rows at any width, so a card
 *  never gets so wide that its score is a long way from its name. */
const MAX_SEAT_COLS = 4;
/** Below this width the viewer is a phone's: two cards a row, no owners, the timeline on its own
 *  row. The viewer's own width, not the window's, because a host column can be narrow on a desk. */
export const NARROW = 640;
/** How wide a thumb has to be before its score chip fits beside the board it describes. */
export const CHIP_MIN = 160;
/** How many turns a tile's hover preview plays: the end of the match, where the result is. */
export const PREVIEW_TURNS = 40;

// Inline SVG rather than glyphs: "⏮" renders as a different width, weight and baseline on every
// platform, and transport controls that jump about are the first thing that makes a player feel
// unfinished.
const ICON = {
  play: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z"/></svg>',
  pause: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="2.5" width="3" height="11" rx="1"/><rect x="9" y="2.5" width="3" height="11" rx="1"/></svg>',
  // One arrow steps; two arrows against a bar go to the end. At fifteen pixels a triangle with a
  // bar beside it and a bare triangle are the same button drawn twice.
  prev: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M11.4 3.2v9.6L4.6 8z"/></svg>',
  next: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M4.6 3.2v9.6L11.4 8z"/></svg>',
  first: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M13 3.6v8.8L9 8z"/><path d="M8.8 3.6v8.8L4.8 8z"/><rect x="2.7" y="3.2" width="1.7" height="9.6" rx=".85"/></svg>',
  last: '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M3 3.6v8.8L7 8z"/><path d="M7.2 3.6v8.8L11.2 8z"/><rect x="11.6" y="3.2" width="1.7" height="9.6" rx=".85"/></svg>',
  plus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="7.2" y="3" width="1.6" height="10" rx=".8"/><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  minus: '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="7.2" width="10" height="1.6" rx=".8"/></svg>',
  fit: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M2.8 6V2.8H6M10 2.8h3.2V6M13.2 10v3.2H10M6 13.2H2.8V10"/></svg>',
  // An eye, because a seat's territory is what it has seen.
  explored: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M1.6 8s2.4-4.3 6.4-4.3S14.4 8 14.4 8s-2.4 4.3-6.4 4.3S1.6 8 1.6 8z"/><circle cx="8" cy="8" r="2" fill="currentColor" stroke="none"/></svg>',
  // The tray's eye again, at the size of the card's figures: the share beside it is what that
  // button draws.
  seen: '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M1.6 8s2.4-4.3 6.4-4.3S14.4 8 14.4 8s-2.4 4.3-6.4 4.3S1.6 8 1.6 8z"/><circle cx="8" cy="8" r="2" fill="currentColor" stroke="none"/></svg>',
  // Arrows out of the corners to fill the screen, into them to leave it: the tray's fit button is
  // the bare corners, and the two sit a few centimetres apart.
  full: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 6.2V2.5h3.7M2.5 2.5l3.8 3.8M13.5 6.2V2.5H9.8M13.5 2.5 9.7 6.3M2.5 9.8v3.7h3.7M2.5 13.5l3.8-3.8M13.5 9.8v3.7H9.8M13.5 13.5 9.7 9.7"/></svg>',
  unfull: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6.3 2.5v3.8H2.5M9.7 2.5v3.8h3.8M6.3 13.5V9.7H2.5M9.7 13.5V9.7h3.8"/></svg>',
  keys: '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><rect x="1.5" y="4" width="13" height="8.5" rx="1.6"/><path d="M4.2 7h.6M7.2 7h.6M10.2 7h.6M4.8 9.8h6.4"/></svg>',
};

/** What the key list says, in the order a reader reaches for them. */
const KEYS = [
  ["space", "play, pause"],
  ["← →", "a turn (shift: ten)"],
  ["↑ ↓", "ten turns"],
  ["Home End", "first, last"],
  ["+ − 0", "zoom, fit"],
  ["E", "territory"],
  ["F", "fullscreen"],
  ["?", "this list"],
];

export class Viewer {
  /**
   * @param {HTMLElement} el   where to draw
   * @param {object|null} replay  the envelope: it carries its own board. May be null for a tile or
   *                           a thumb given `frame` or `board` instead
   * @param {object} [opts]    { tier, frame, board, turn, from, to, autoplay, speed, theme, chrome,
   *                             height, stageHeight, labels, explored, onTurn }
   */
  constructor(el, replay, opts = {}) {
    this.el = el;
    this.replay = replay ?? null;
    this.opts = opts;
    this.onTurn = opts.onTurn;
    this.tier = TIERS.includes(opts.tier) ? opts.tier : "stage";
    this.playing = false;
    this.speed = opts.speed ?? 1;
    this.raf = null;
    this.peekTimer = null;
    this.destroyed = false;
    this.listeners = { turn: new Set(), layout: new Set() };
    this.previewToken = 0;

    // A host bug, said as one, rather than a player with nothing in it. A stage or a player IS a
    // replay; a tile draws names, and without an envelope nothing here knows any.
    if (FULL.has(this.tier) && !this.replay) throw new Error(`a ${this.tier} needs a replay`);
    if (this.tier === "tile" && !this.replay && !Array.isArray(opts.labels)) {
      throw new Error("a tile with no replay needs `labels`: nothing else names its seats");
    }

    injectCss(el.ownerDocument);
    el.innerHTML = "";
    el.classList.add("tb-viz", `tb-tier-${this.tier}`);
    if (opts.theme) el.dataset.tbTheme = opts.theme;
    if (opts.chrome) el.dataset.tbChrome = opts.chrome;
    // A host that says how tall the player is says it once, here, rather than having to know that
    // the root is a flex column that will otherwise collapse to its bar. `stageHeight` says how tall
    // the BOARD is instead, and the player is that plus its bars -- which is what a host sizing the
    // board to the screen means, since the seat cards are one row or several depending on the
    // width. Given both, the board's wins: `height` is then a fallback for a viewer from before it.
    // A tile and a thumb are boxes whose shape the host sets, and take neither.
    const len = (v) => (typeof v === "number" ? `${v}px` : v);
    if (FULL.has(this.tier) && opts.height && !opts.stageHeight) el.style.height = len(opts.height);

    let source;
    try {
      source = sourceOf(this.tier, this.replay, opts);
    } catch (e) {
      el.innerHTML = `<div class="tb-err">This replay could not be decoded.<br>${esc(e.message)}</div>`;
      return;
    }
    // Where the frames came from: the whole replay, one stored frame, or a board's turn zero. Only
    // the first has a match's worth of anything; the last has no scores yet.
    this.from = source.kind;
    this.frames = source.frames;
    this.map = source.map;

    // A range narrows what the timeline covers without changing what a turn number means, so a
    // tutorial can point at turns 40-60 of a real match and the reader still sees "turn 47". A tile
    // or a thumb rests on the last frame it has: a finished match is shown as it finished.
    const last = this.frames.length - 1;
    this.lo = clamp(opts.from ?? 0, 0, last);
    this.hi = clamp(opts.to ?? last, this.lo, last);
    this.i = clamp(opts.turn ?? (FULL.has(this.tier) ? this.lo : this.hi), this.lo, this.hi);
    this.rest = { frames: this.frames, lo: this.lo, hi: this.hi, i: this.i };

    this.labels = seatLabels(this.replay ?? {}, this.frames[0].score.length, opts.labels);
    this.names = this.labels.map((l) => l.name);
    this.events = findEvents(this.frames, this.lo, this.hi);
    // Derived from the frames on first use and kept: the board's step counts on the first paint,
    // the territory the first time someone asks to see it.
    this.board = null;
    this.firstSeen = null;
    // Only a cartridge that reports what each seat saw can draw it. This bundle ships with one that
    // does; a shell copied beside an older engine would otherwise draw the whole board as fog.
    this.canExplore = this.tier === "stage" && Array.isArray(this.frames[0].discovered);
    this.explored = Boolean(opts.explored) && this.canExplore;

    this.build();
    this.renderer.setBoard(this.map);
    this.observe();
    // A tutorial points at a turn AND a place: `zoom` is a multiple of the fitted scale, `centre`
    // is the cell to put in the middle. Applied after the first layout, because both are relative
    // to a viewport that does not exist until then. Only a stage zooms.
    if (this.tier === "stage" && opts.zoom && opts.zoom > 1) {
      this.renderer.zoomAt(opts.zoom, 0, 0);
      const c = opts.centre ?? this.busiestCell();
      this.renderer.centreOn(c[0], c[1]);
    } else if (this.tier === "stage" && opts.centre) {
      this.renderer.centreOn(opts.centre[0], opts.centre[1]);
    }
    this.show();
    if (opts.autoplay && FULL.has(this.tier)) this.play();
  }

  // ------------------------------------------------------------------ the public surface
  //
  // What a host -- and graph.js -- may read and call. Everything else on this class is the
  // viewer's own and may change without notice.

  /** The turn shown. In a replay a frame's index and its turn are the same number. */
  get turn() {
    return this.frames?.[this.i]?.turn ?? 0;
  }

  /** The turns the timeline covers: `from` and `to`, or the whole match. */
  get range() {
    if (!this.frames) return { lo: 0, hi: 0 };
    return { lo: this.frames[this.lo].turn, hi: this.frames[this.hi].turn };
  }

  /** Show a turn, clamped to the range. Does not pause: a host scrubbing decides that. */
  seek(turn) {
    if (!this.frames) return;
    this.goto(Number(turn) - this.frames[0].turn);
  }

  /**
   * Each seat's ants alive, hills standing and score at every turn: `{ ants, hills, score }`, each
   * `[seat][turn]`. Counted once from the frames and kept, so a graph reads a thousand turns of
   * eight seats as array lookups.
   */
  series() {
    if (!this.frames) return { ants: [], hills: [], score: [] };
    if (this.seriesCache?.frames === this.frames) return this.seriesCache.value;
    const n = this.frames[0].score.length;
    const T = this.frames.length;
    const blank = () => Array.from({ length: n }, () => new Array(T).fill(0));
    const value = { ants: blank(), hills: blank(), score: blank() };
    this.frames.forEach((f, t) => {
      for (const a of f.ants) if (a[2] < n) value.ants[a[2]][t]++;
      for (const h of f.hills) if (h[2] < n) value.hills[h[2]][t]++;
      for (let s = 0; s < n; s++) value.score[s][t] = f.score[s];
    });
    this.seriesCache = { frames: this.frames, value };
    return value;
  }

  /**
   * Listen for `"turn"` (called with the turn and its frame, each time one is shown) or `"layout"`
   * (called with `trackBox()` each time the timeline may have moved). Returns the unsubscribe.
   * `onTurn` stays the host's single callback; this is for anything else that follows the player.
   */
  on(type, fn) {
    const set = this.listeners[type];
    if (!set) throw new Error(`no such event: ${type}`);
    set.add(fn);
    return () => set.delete(fn);
  }

  /**
   * Where the timeline's track is, in page pixels: `{ left, width }`, or null in a tier that has
   * none. The ants graph lines its x-axis up with it, so a tick in one sits over the same tick in
   * the other; it moves when the viewer is resized, and `"layout"` says so.
   */
  trackBox() {
    if (!this.track) return null;
    const r = this.track.getBoundingClientRect();
    const view = this.el.ownerDocument.defaultView;
    return { left: r.left + (view?.scrollX ?? 0), width: r.width };
  }

  /**
   * A tile's hover preview: the last forty turns of its match, played once, ending on the last.
   *
   * The replay is fetched and decoded once and kept, so hovering the same card again costs
   * nothing. The host owns the delay before a hover counts and keeps one preview playing at a time;
   * nothing here ever moves on to another match.
   *
   * @param {object|string} [replay]  the envelope, or a URL to fetch it from; the one the tile was
   *                                  mounted with when omitted
   */
  async preview(replay) {
    if (this.tier !== "tile") throw new Error("preview() is a tile's");
    if (!this.frames) return;
    const token = ++this.previewToken;
    let env = replay ?? this.previewEnv ?? this.replay;
    if (typeof env === "string") {
      if (env === this.previewUrl && this.previewEnv) env = this.previewEnv;
      else {
        const url = env;
        env = await (await fetch(url)).json();
        this.previewUrl = url;
      }
    }
    if (!env) throw new Error("preview() needs a replay");
    this.previewEnv = env;
    // Stopped, or asked again, while it fetched: this one is no longer wanted.
    if (token !== this.previewToken || this.destroyed) return;
    if (this.previewFor !== env) {
      const [from, to] = previewRange(Number(env.turns ?? 0));
      this.previewFrames = framesBetween(env, from, to);
      this.previewFor = env;
    }
    this.pause();
    this.frames = this.previewFrames;
    this.lo = 0;
    this.hi = this.frames.length - 1;
    this.i = 0;
    this.show();
    this.play();
  }

  /** Stop a preview and go back to the frame the tile rests on. */
  stop() {
    this.previewToken++;
    this.pause();
    if (!this.rest || this.frames === this.rest.frames) return;
    ({ frames: this.frames, lo: this.lo, hi: this.hi, i: this.i } = this.rest);
    this.show();
  }

  /**
   * Fill the screen with the viewer, or leave it. A player is promoted to a stage for as long as
   * it lasts -- the same element, the same canvas, the same frames, with the stage's parts built
   * the first time -- and goes back to a player after.
   */
  fullscreen(on = !this.isFull()) {
    if (!FULL.has(this.tier) || !this.frames) return;
    const d = this.el.ownerDocument;
    if (on) {
      if (this.isFull()) return;
      if (this.tier === "player") {
        this.promoted = true;
        this.setTier("stage");
      }
      // Where the browser refuses -- a phone has no fullscreen for an element, and a call outside
      // a click is refused everywhere -- the viewer fills the window instead.
      const req = this.el.requestFullscreen?.();
      if (req && typeof req.catch === "function") req.catch(() => this.fillWindow(true));
      else if (!req) this.fillWindow(true);
      this.writeFull();
    } else if (d.fullscreenElement === this.el) {
      d.exitFullscreen?.(); // fullscreenchange does the rest
    } else {
      this.fillWindow(false);
      this.leftFullscreen();
    }
  }

  // ------------------------------------------------------------------ building

  make(tag, cls, parent, html) {
    const n = this.el.ownerDocument.createElement(tag);
    if (cls) n.className = cls;
    if (html != null) n.innerHTML = html;
    (parent || this.el).appendChild(n);
    return n;
  }

  button(parent, icon, title, fn, cls) {
    const b = this.make("button", cls, parent, icon);
    b.type = "button";
    b.title = title;
    b.setAttribute("aria-label", title);
    b.onclick = fn;
    return b;
  }

  build() {
    const full = FULL.has(this.tier);

    // ---- the seat cards: every seat, always on screen
    if (full) this.seats = this.make("div", "tb-top");

    // ---- stage
    this.stage = this.make("div", "tb-stage");
    if (full && this.opts.stageHeight) {
      this.stage.style.flex = "none";
      this.stage.style.height =
        typeof this.opts.stageHeight === "number" ? `${this.opts.stageHeight}px` : this.opts.stageHeight;
    }
    this.canvas = this.make("canvas", null, this.stage);
    this.canvas.setAttribute("role", "img");
    this.renderer = new Renderer(this.canvas);

    if (full) {
      this.buildBar();
      this.buildSeats();
      if (this.tier === "stage") this.buildStageParts();
      // ---- input. A player takes the transport's keys; zoom, pan and territory are the stage's,
      // and each handler asks which tier it is in, since a player can become a stage.
      this.el.tabIndex = 0;
      this.el.addEventListener("keydown", (e) => this.key(e));
      this.stage.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
      this.stage.addEventListener("pointerdown", (e) => this.onDown(e));
      this.stage.addEventListener("pointermove", (e) => this.onMove(e));
      this.stage.addEventListener("pointerup", (e) => this.onUp(e));
      this.onFullChange = () => {
        if (this.el.ownerDocument.fullscreenElement !== this.el && !this.el.dataset.tbFull) {
          this.leftFullscreen();
        }
        this.writeFull();
      };
      this.el.ownerDocument.addEventListener("fullscreenchange", this.onFullChange);
    } else if (this.tier === "tile") {
      this.buildOverlay();
    } else {
      this.buildChips();
    }
  }

  /** The transport: first, previous, play, next, last, the timeline, the turn, and fullscreen. */
  buildBar() {
    const bar = this.make("div", "tb-bar");
    this.bar = bar;
    const t = this.make("div", "tb-transport", bar);
    this.firstBtn = this.button(t, ICON.first, "First turn (Home)", () => this.goto(this.lo));
    this.prevBtn = this.button(t, ICON.prev, "Previous turn (←)", () => this.step(-1));
    this.playBtn = this.button(t, ICON.play, "Play (space)", () => (this.playing ? this.pause() : this.play()), "tb-play");
    this.nextBtn = this.button(t, ICON.next, "Next turn (→)", () => this.step(1));
    this.lastBtn = this.button(t, ICON.last, "Last turn (End)", () => this.goto(this.hi));

    this.buildTrack(this.make("div", "tb-track", bar));
    this.turnLabel = this.make("span", "tb-turn", bar);
    this.end = this.make("div", "tb-end", bar);
    this.fullBtn = this.button(this.end, ICON.full, "Fullscreen (F)", () => this.fullscreen(), "tb-full");
    this.fullBtn.setAttribute("aria-pressed", "false");
  }

  /**
   * What only a stage has: the tray with zoom and territory, playback speed, and the key list.
   * Built with a stage, or the first time a player is promoted to one, and never twice.
   */
  buildStageParts() {
    if (this.stageBuilt) return;
    this.stageBuilt = true;

    // ---- the tray: the tools, over the board's top-right corner
    const tray = this.make("div", "tb-tray tb-so", this.stage);
    const head = this.make("div", "tb-row tb-head", tray);
    const tools = this.make("div", "tb-tools tb-pane", head);
    this.exploreBtn = this.button(tools, ICON.explored, "Explored territory (E)", () =>
      this.setExplored(!this.explored)
    );
    this.canExplore = Array.isArray(this.frames[0].discovered);
    this.exploreBtn.setAttribute("aria-pressed", String(this.explored));
    if (!this.canExplore) {
      this.exploreBtn.disabled = true;
      this.exploreBtn.title = "This engine does not report what each seat has seen";
    }
    this.make("span", "tb-sep", tools);
    this.button(tools, ICON.minus, "Zoom out (−)", () => this.zoom(1 / 1.4));
    this.button(tools, ICON.plus, "Zoom in (+)", () => this.zoom(1.4));
    this.button(tools, ICON.fit, "Fit the board (0)", () => {
      this.renderer.fit();
      this.paint();
    });

    // ---- the key list, over the board's bottom-left corner
    this.keyList = this.make("dl", "tb-keys tb-pane tb-so", this.stage);
    this.keyList.hidden = true;
    for (const [k, what] of KEYS) {
      this.make("dt", null, this.keyList).textContent = k;
      this.make("dd", null, this.keyList).textContent = what;
    }

    // ---- speed and the key list's button, before fullscreen at the transport's end
    const d = this.el.ownerDocument;
    this.speedBtn = d.createElement("button");
    this.speedBtn.className = "tb-speed tb-so";
    this.speedBtn.type = "button";
    this.speedBtn.title = "Playback speed";
    this.speedBtn.onclick = () => {
      this.speed = SPEEDS[(SPEEDS.indexOf(this.speed) + 1) % SPEEDS.length];
      this.speedBtn.textContent = `${this.speed}×`;
    };
    this.speedBtn.textContent = `${this.speed}×`;
    this.end.insertBefore(this.speedBtn, this.fullBtn);
    this.keysBtn = d.createElement("button");
    this.keysBtn.className = "tb-keys-btn tb-so";
    this.keysBtn.type = "button";
    this.keysBtn.title = "Keyboard shortcuts (?)";
    this.keysBtn.setAttribute("aria-label", "Keyboard shortcuts (?)");
    this.keysBtn.setAttribute("aria-pressed", "false");
    this.keysBtn.innerHTML = ICON.keys;
    this.keysBtn.onclick = () => this.showKeys(this.keyList.hidden);
    this.end.insertBefore(this.keysBtn, this.fullBtn);
  }

  /**
   * The seat cards, built once and then written to.
   *
   * They used to be rebuilt from scratch on every frame, which is ten times a second at 1× and
   * eighty at 8×: a row of elements thrown away and remade while the reader is trying to read the
   * numbers on it. Nothing about a seat changes but its score and its counts.
   */
  buildSeats() {
    const d = this.el.ownerDocument;
    const span = (parent, cls, text) => {
      const n = d.createElement("span");
      n.className = cls;
      if (text != null) {
        n.textContent = text;
        n.title = text;
      }
      parent.appendChild(n);
      return n;
    };
    // A count is a shape and a number. The shape is an image with a name, so a screen reader says
    // "ants 12" where the eye reads a dot and a 12.
    const count = (parent, kind, shape, label, html) => {
      const n = span(parent, `tb-n tb-n-${kind}`);
      const s = span(n, shape);
      s.setAttribute("role", "img");
      s.setAttribute("aria-label", label);
      if (html) s.innerHTML = html;
      return { n, value: n.appendChild(d.createElement("b")) };
    };
    // How many hills each seat started with, from the board: the squares a razed hill leaves
    // hollow. A frame lists only the hills still standing, so the board is the only place the
    // razed ones are still written down.
    const started = hillsPerSeat(this.map, this.labels.length);
    const standing = this.series().hills;
    this.seatRows = this.labels.map(({ name, by }, seat) => {
      const row = d.createElement("div");
      row.className = "tb-seat";
      row.style.setProperty("--tb-seat", SEATS[seat % SEATS.length]);
      const id = span(row, "tb-id");
      span(id, "tb-name", name);
      if (by) span(id, "tb-by", by);
      const score = span(row, "tb-score");
      score.title = "score";
      const nums = span(row, "tb-nums");
      const ants = count(nums, "ants", "tb-ant", "ants");
      const hills = span(nums, "tb-n tb-hills");
      hills.setAttribute("role", "img");
      const most = Math.max(started[seat], ...standing[seat]);
      const squares = Array.from({ length: most }, () => span(hills, "tb-hill"));
      // Territory is a toggle, so its share is built with the rest and shown only while it is on.
      const seen = count(nums, "seen", "tb-eye", "explored", ICON.seen);
      this.seats.appendChild(row);
      return { row, score, nums, ants: ants.value, hills, squares, seen: seen.n, pct: seen.value };
    });
    // One row until the first layout says otherwise: see seatColumns().
    this.seatCols = this.seatRows.length;
    this.seats.style.setProperty("--tb-cols", String(this.seatCols));
  }

  /**
   * A tile's overlay: the seats' names and scores, and the turn. Two seats sit in the top corners,
   * mirrored like a scoreboard; three to eight are the two leading seats and how many more there
   * are. A layer of its own over the canvas, so a long name is cut in the overlay and never moves
   * the board.
   */
  buildOverlay() {
    const over = this.make("div", "tb-over", this.el);
    this.overlay = over;
    const n = this.labels.length;
    const tag = (parent, cls) => {
      const t = this.make("span", `tb-tag ${cls}`, parent);
      const sw = this.make("i", null, t);
      const nm = this.make("span", "tb-nm", t);
      const sc = this.make("span", "tb-sc", t);
      return { t, sw, nm, sc };
    };
    if (n === 2) {
      this.tags = [tag(over, "tb-corner tb-l"), tag(over, "tb-corner tb-r")];
    } else {
      const list = this.make("span", "tb-list", over);
      this.tags = Array.from({ length: Math.min(n, 2) }, () => tag(list, ""));
      if (n > 2) this.make("span", "tb-more", list).textContent = `+${n - 2}`;
    }
    this.turnTag = this.make("span", "tb-at", over);
    this.turnTag.title = "turn";
  }

  /** A thumb's score chip: shown once the box is wide enough to hold it beside the board. */
  buildChips() {
    this.chips = this.make("span", "tb-chips", this.el);
    this.chips.hidden = true;
    const n = this.labels.length;
    this.chipTags = Array.from({ length: Math.min(n, 2) }, () => {
      const s = this.make("span", "tb-chip", this.chips);
      return { sw: this.make("i", null, s), nm: this.make("span", "tb-nm", s), sc: this.make("span", null, s) };
    });
    if (n > 2) this.make("span", "tb-more", this.chips).textContent = `+${n - 2}`;
  }

  buildTrack(track) {
    const d = this.el.ownerDocument;
    this.track = track;
    track.setAttribute("role", "slider");
    track.setAttribute("aria-label", "Turn");
    // Focusable, so it is a real operable slider: the root's keydown handler (Arrow/Home/End)
    // fires for the focused track by bubbling, so no key logic is duplicated here.
    track.setAttribute("tabindex", "0");
    const rail = d.createElement("div");
    rail.className = "tb-rail";
    track.appendChild(rail);
    this.fill = d.createElement("div");
    this.fill.className = "tb-fill";
    track.appendChild(this.fill);

    // Event marks: where a hill fell and where a colony ended. A two-hundred-turn match has three
    // or four moments in it, and without these they can only be found by scrubbing for them.
    for (const ev of this.events) {
      const m = d.createElement("div");
      m.className = "tb-mark";
      m.dataset.kind = ev.kind;
      m.style.left = `${this.pct(ev.i)}%`;
      m.style.background = SEATS[ev.seat % SEATS.length];
      m.title = `turn ${ev.turn}: ${this.names[ev.seat]}, ${ev.what}`;
      track.appendChild(m);
    }

    this.thumb = d.createElement("div");
    this.thumb.className = "tb-thumb";
    track.appendChild(this.thumb);

    // Click anywhere on the track to go there; drag to scrub.
    const at = (e) => {
      const r = track.getBoundingClientRect();
      const f = clamp((e.clientX - r.left) / Math.max(1, r.width), 0, 1);
      return Math.round(this.lo + f * (this.hi - this.lo));
    };
    track.addEventListener("pointerdown", (e) => {
      track.setPointerCapture(e.pointerId);
      this.scrubbing = true;
      this.pause();
      this.goto(at(e));
    });
    track.addEventListener("pointermove", (e) => {
      if (this.scrubbing) this.goto(at(e));
    });
    const end = (e) => {
      if (!this.scrubbing) return;
      this.scrubbing = false;
      try {
        track.releasePointerCapture(e.pointerId);
      } catch {}
    };
    track.addEventListener("pointerup", end);
    track.addEventListener("pointercancel", end);
  }

  observe() {
    this.fit = () => {
      if (this.destroyed) return;
      // The seats first: the rows they take are what the stage has left.
      const w = this.el.clientWidth;
      if (w >= 2 && this.seatRows) this.layoutSeats(w);
      if (this.chips) this.chips.hidden = !(w >= CHIP_MIN) || this.from === "board";
      const r = this.stage.getBoundingClientRect();
      if (r.width >= 2 && r.height >= 2) {
        this.renderer.resize(r.width, r.height);
        this.paint();
      }
      if (this.track) this.emit("layout", this.trackBox());
    };
    this.ro = new ResizeObserver(this.fit);
    this.ro.observe(this.stage);
    // The track moves when the buttons beside it change as well as when the viewer resizes.
    if (this.track) this.ro.observe(this.track);
    this.fit();
  }

  /**
   * Lay the seat cards out for a width -- see seatColumns() -- writing only a change. Below
   * `NARROW` the root says so, and the stylesheet drops the owners and gives the timeline a row.
   */
  layoutSeats(width) {
    const narrow = width < NARROW;
    if (narrow !== Boolean(this.el.dataset.tbNarrow)) {
      if (narrow) this.el.dataset.tbNarrow = "1";
      else delete this.el.dataset.tbNarrow;
    }
    const cols = seatColumns(this.seatRows.length, width);
    if (cols === this.seatCols) return;
    this.seatCols = cols;
    this.seats.style.setProperty("--tb-cols", String(cols));
  }

  /**
   * Switch between a stage and a player in place: the same element, canvas and frames, only the
   * parts shown change. The stage's own are built the first time they are wanted.
   */
  setTier(tier) {
    if (!FULL.has(tier) || !FULL.has(this.tier) || tier === this.tier) return;
    this.el.classList.remove(`tb-tier-${this.tier}`);
    this.tier = tier;
    this.el.classList.add(`tb-tier-${tier}`);
    if (tier === "stage") {
      this.buildStageParts();
    } else {
      // A player neither zooms nor draws territory, so it comes back as it was.
      this.showKeys(false);
      if (this.explored) this.setExplored(false);
      this.renderer.fit();
    }
    this.fit();
    this.writeSeats();
  }

  isFull() {
    return this.el.ownerDocument.fullscreenElement === this.el || Boolean(this.el.dataset.tbFull);
  }

  fillWindow(on) {
    if (on) this.el.dataset.tbFull = "1";
    else delete this.el.dataset.tbFull;
    this.writeFull();
    if (this.fit) this.fit();
  }

  leftFullscreen() {
    if (this.promoted) {
      this.promoted = false;
      this.setTier("player");
    }
    this.writeFull();
  }

  writeFull() {
    if (!this.fullBtn) return;
    const on = this.isFull();
    this.fullBtn.innerHTML = on ? ICON.unfull : ICON.full;
    const title = on ? "Leave fullscreen (F)" : "Fullscreen (F)";
    this.fullBtn.title = title;
    this.fullBtn.setAttribute("aria-label", title);
    this.fullBtn.setAttribute("aria-pressed", String(on));
  }

  showKeys(on) {
    if (!this.keyList) return;
    this.keyList.hidden = !on;
    this.keysBtn.setAttribute("aria-pressed", String(Boolean(on)));
  }

  /**
   * Where the action is: the densest cluster of ants, not their centre of mass.
   *
   * The mean is the wrong answer and wrong in the way that matters -- two colonies on opposite
   * sides of a board average to the empty middle, so a zoom that used it would open on nothing at
   * all. This buckets the board and takes the fullest bucket.
   */
  busiestCell() {
    const f = this.frames[this.i];
    const all = f.ants.length ? f.ants : f.hills;
    if (!all.length) return [this.renderer.rows / 2, this.renderer.cols / 2];
    const step = 8;
    const buckets = new Map();
    for (const a of all) {
      const k = `${(a[0] / step) | 0},${(a[1] / step) | 0}`;
      const b = buckets.get(k) ?? { n: 0, r: 0, c: 0 };
      b.n++;
      b.r += a[0];
      b.c += a[1];
      buckets.set(k, b);
    }
    let best = null;
    for (const b of buckets.values()) if (!best || b.n > best.n) best = b;
    return [best.r / best.n, best.c / best.n];
  }

  // ------------------------------------------------------------------ input

  key(e) {
    const k = e.key;
    const jump = e.shiftKey ? 10 : 1;
    const stage = this.tier === "stage";
    const map = {
      " ": () => (this.playing ? this.pause() : this.play()),
      ArrowRight: () => this.step(jump),
      ArrowLeft: () => this.step(-jump),
      ArrowUp: () => this.step(10),
      ArrowDown: () => this.step(-10),
      Home: () => this.goto(this.lo),
      End: () => this.goto(this.hi),
      f: () => this.fullscreen(),
      F: () => this.fullscreen(),
    };
    if (stage) {
      Object.assign(map, {
        "+": () => this.zoom(1.4),
        "=": () => this.zoom(1.4),
        "-": () => this.zoom(1 / 1.4),
        0: () => {
          this.renderer.fit();
          this.paint();
        },
        e: () => this.setExplored(!this.explored),
        E: () => this.setExplored(!this.explored),
        "?": () => this.showKeys(this.keyList.hidden),
      });
    }
    // Escape closes the key list first, then leaves a window-filling fullscreen; the browser's
    // own fullscreen takes Escape before a page ever sees it.
    if (k === "Escape") {
      if (this.keyList && !this.keyList.hidden) this.showKeys(false);
      else if (this.el.dataset.tbFull) this.fullscreen(false);
      return;
    }
    const fn = map[k];
    if (!fn) return;
    e.preventDefault();
    // The territory is worth watching grow, so turning it on is not a reason to stop, and neither
    // is filling the screen or reading the key list.
    if (!" eEfF?".includes(k)) this.pause();
    fn();
  }

  /**
   * Show the tray for a moment.
   *
   * Hover is what raises it, and a touch screen has no hover: without this the zoom buttons and the
   * territory toggle would be unreachable on a phone rather than merely out of the way.
   */
  peek() {
    if (this.destroyed) return;
    this.el.dataset.tbPeek = "1";
    if (this.peekTimer) clearTimeout(this.peekTimer);
    this.peekTimer = setTimeout(() => {
      delete this.el.dataset.tbPeek;
      this.peekTimer = null;
    }, PEEK_MS);
  }

  onWheel(e) {
    // A player's board is looked at, and a wheel over it scrolls the page it sits in.
    if (this.tier !== "stage") return;
    e.preventDefault();
    const r = this.canvas.getBoundingClientRect();
    this.renderer.zoomAt(e.deltaY < 0 ? 1.12 : 1 / 1.12, e.clientX - r.left, e.clientY - r.top);
    this.paint();
  }

  onDown(e) {
    if (this.tier !== "stage") return;
    if (e.pointerType === "touch") this.peek();
    if (e.target !== this.canvas && e.target !== this.stage) return;
    this.stage.setPointerCapture(e.pointerId);
    this.drag = { x: e.clientX, y: e.clientY };
    this.stage.classList.add("tb-drag");
  }

  onMove(e) {
    if (!this.drag) return;
    const dx = e.clientX - this.drag.x;
    const dy = e.clientY - this.drag.y;
    this.drag.x = e.clientX;
    this.drag.y = e.clientY;
    this.renderer.pan(dx, dy);
    this.paint();
  }

  onUp(e) {
    this.drag = null;
    this.stage.classList.remove("tb-drag");
    try {
      this.stage.releasePointerCapture(e.pointerId);
    } catch {}
  }

  zoom(f) {
    if (this.tier !== "stage") return;
    const { w, h } = this.renderer.viewport();
    this.renderer.zoomAt(f, w / 2, h / 2);
    this.paint();
  }

  // ------------------------------------------------------------------ playback

  step(n) {
    this.goto(this.i + n);
  }

  /** Show frame `i` of the frames held, clamped to the range. */
  goto(i) {
    const next = clamp(i, this.lo, this.hi);
    if (next === this.i) return;
    this.i = next;
    this.show();
  }

  pct(i) {
    const span = Math.max(1, this.hi - this.lo);
    return ((i - this.lo) / span) * 100;
  }

  paint() {
    this.renderer.render(this.frames[this.i], { threats: this.threatsAt(this.i) });
  }

  show() {
    const f = this.frames[this.i];
    if (this.explored) this.renderer.setTerritory(this.territoryAt(this.i).mask);
    this.paint();

    if (this.track) {
      const p = this.pct(this.i);
      this.fill.style.width = `${p}%`;
      this.thumb.style.left = `${p}%`;
      this.track.setAttribute("aria-valuenow", String(f.turn));
      this.track.setAttribute("aria-valuemin", String(this.frames[this.lo].turn));
      this.track.setAttribute("aria-valuemax", String(this.frames[this.hi].turn));
      this.turnLabel.textContent = `${f.turn} / ${this.frames[this.hi].turn}`;

      this.prevBtn.disabled = this.i <= this.lo;
      this.firstBtn.disabled = this.i <= this.lo;
      this.nextBtn.disabled = this.i >= this.hi;
      this.lastBtn.disabled = this.i >= this.hi;
    }

    this.describe();
    if (this.seatRows) this.writeSeats();
    if (this.overlay) this.writeOverlay();
    if (this.chips) this.writeChips();

    for (const fn of this.listeners.turn) fn(f.turn, f);
    if (this.onTurn) this.onTurn(f);
  }

  /**
   * What the canvas says to a screen reader: the turn, each seat's ants and score, what the turn
   * did (who lost ants, to whom; whose hill fell), and the threats.
   */
  describe() {
    const f = this.frames[this.i];
    const ants = antsBySeat(f);
    const threats = this.threatsAt(this.i).map(
      (t) => `${this.names[t.owner]}'s hill has an enemy ${t.steps} move${t.steps === 1 ? "" : "s"} from it`
    );
    this.canvas.setAttribute(
      "aria-label",
      `Turn ${f.turn}. ${f.score
        .map((s, i) =>
          this.from === "board"
            ? this.names[i]
            : `${this.names[i]}: ${ants[i]} ant${ants[i] === 1 ? "" : "s"}, score ${s}`
        )
        .concat(describeEvents(f, this.names), threats)
        .join(". ")}`
    );
  }

  /**
   * The seat cards' scores and counts.
   *
   * Apart from `show()` because the territory toggle changes what a card says without changing the
   * turn, and `show()` also tells the host that the turn changed.
   */
  writeSeats() {
    const f = this.frames[this.i];
    // One pass for the counts every card wants, rather than one pass per seat.
    const ants = antsBySeat(f);
    const hills = new Array(f.score.length).fill(0);
    for (const h of f.hills) hills[h[2]] = (hills[h[2]] ?? 0) + 1;
    const seen = this.explored ? this.territoryAt(this.i).seen : null;
    const cells = this.renderer.rows * this.renderer.cols;

    f.score.forEach((score, seat) => {
      const row = this.seatRows[seat];
      if (!row) return;
      const pct = seen ? Math.round((100 * seen[seat]) / cells) : 0;
      row.row.dataset.out = String(ants[seat] === 0);
      row.score.textContent = String(score);
      row.ants.textContent = String(ants[seat]);
      row.squares.forEach((sq, k) => {
        if (k < hills[seat]) sq.dataset.on = "";
        else delete sq.dataset.on;
      });
      const razed = Math.max(0, row.squares.length - hills[seat]);
      row.hills.setAttribute("aria-label", `hills: ${hills[seat]} standing, ${razed} razed`);
      row.seen.hidden = !seen;
      row.pct.textContent = `${pct}%`;
      // The words the shapes stand for, on hover.
      row.nums.title =
        `${ants[seat]} ant${ants[seat] === 1 ? "" : "s"} · ${hills[seat]} hill${hills[seat] === 1 ? "" : "s"}` +
        (razed ? ` (${razed} razed)` : "") +
        (seen ? ` · ${pct}% explored` : "");
    });
  }

  /** The tile's names, scores and turn, for the frame shown. */
  writeOverlay() {
    const f = this.frames[this.i];
    const who = this.labels.length === 2 ? [0, 1] : leaders(f.score, 2);
    this.tags.forEach((tag, k) => {
      const seat = who[k];
      tag.t.style.setProperty("--tb-seat", SEATS[seat % SEATS.length]);
      tag.nm.textContent = this.names[seat];
      tag.t.title = this.labels[seat].by ? `${this.names[seat]} ${this.labels[seat].by}` : this.names[seat];
      // A board nobody has played on yet has no scores, only the points each seat starts with.
      tag.sc.textContent = this.from === "board" ? "—" : String(f.score[seat]);
    });
    this.turnTag.hidden = this.from === "board";
    this.turnTag.textContent = String(f.turn);
  }

  writeChips() {
    const f = this.frames[this.i];
    const who = this.labels.length === 2 ? [0, 1] : leaders(f.score, 2);
    this.chipTags.forEach((c, k) => {
      c.sw.style.setProperty("--tb-seat", SEATS[who[k] % SEATS.length]);
      c.nm.textContent = this.names[who[k]];
      c.sc.textContent = String(f.score[who[k]]);
    });
  }

  /** Show or hide each seat's explored territory. */
  setExplored(on) {
    this.explored = Boolean(on) && this.canExplore && this.tier === "stage";
    if (this.exploreBtn) this.exploreBtn.setAttribute("aria-pressed", String(this.explored));
    this.renderer.setTerritory(this.explored ? this.territoryAt(this.i).mask : null);
    this.paint();
    this.writeSeats();
  }

  /**
   * The hills an enemy ant is within `THREAT_STEPS` moves of at frame `i`, and how near the nearest
   * one is. Kept for the one frame, because `paint()` runs on every pan and zoom as well as every
   * turn. A tile and a thumb draw no rings: at their scale a ring is a smudge over the board.
   */
  threatsAt(i) {
    if (!FULL.has(this.tier)) return [];
    if (this.threatCache?.i === i && this.threatCache.frames === this.frames) return this.threatCache.threats;
    const map = this.map;
    if (!this.board && map) {
      const water = expandRle(map.water, map.rows * map.cols);
      this.board = { rows: map.rows, cols: map.cols, water, steps: new Map() };
    }
    // On a board the reach would cover, a ring says nothing: every hill is always "close", and
    // the rings drawn across the wrap bury a lesson board under arcs. Draw them only where the
    // reach is a region of the board rather than the whole of it.
    const roomy = this.board && Math.min(this.board.rows, this.board.cols) > 2 * THREAT_STEPS + 1;
    const threats = roomy ? threatsIn(this.frames[i], this.board, THREAT_STEPS) : [];
    this.threatCache = { i, frames: this.frames, threats };
    return threats;
  }

  /**
   * What each seat has explored at frame `i`: a byte per cell with bit `s` set where seat `s` knows
   * it, and how many cells each seat knows.
   *
   * The frames' `discovered` -- the engine's own record of what each seat saw first, turn by turn --
   * is folded once into the frame at which each seat first saw each cell. After that any frame's
   * territory is one comparison per cell, which is what lets it keep up with playback at 8×.
   */
  territoryAt(i) {
    if (this.territoryCache?.i === i) return this.territoryCache;
    const cols = this.renderer.cols;
    const cells = this.renderer.rows * cols;
    this.firstSeen ??= firstSeen(this.frames, cols, cells);
    const mask = new Uint8Array(cells);
    const seen = this.firstSeen.map((at, seat) => {
      let n = 0;
      for (let k = 0; k < cells; k++) {
        if (at[k] <= i) {
          mask[k] |= 1 << seat;
          n++;
        }
      }
      return n;
    });
    this.territoryCache = { i, mask, seen };
    return this.territoryCache;
  }

  play() {
    if (this.playing || this.destroyed || !this.frames) return;
    if (this.i >= this.hi) this.goto(this.lo);
    this.playing = true;
    this.writePlay();
    let last = performance.now();
    let acc = 0;
    const tick = (now) => {
      if (!this.playing) return;
      acc += (now - last) * this.speed;
      last = now;
      const per = 1000 / TURNS_PER_SECOND;
      let moved = false;
      while (acc >= per) {
        acc -= per;
        if (this.i >= this.hi) break;
        this.i++;
        moved = true;
      }
      if (moved) this.show();
      // A replay ends on its last frame and stays there. Nothing plays on into another match.
      if (this.i >= this.hi) {
        this.pause();
        return;
      }
      this.raf = requestAnimationFrame(tick);
    };
    this.raf = requestAnimationFrame(tick);
  }

  pause() {
    if (!this.playing) return;
    this.playing = false;
    this.writePlay();
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = null;
  }

  writePlay() {
    if (!this.playBtn) return;
    this.playBtn.innerHTML = this.playing ? ICON.pause : ICON.play;
    const title = this.playing ? "Pause (space)" : "Play (space)";
    this.playBtn.title = title;
    this.playBtn.setAttribute("aria-label", title);
  }

  emit(type, ...args) {
    for (const fn of this.listeners[type]) fn(...args);
  }

  destroy() {
    this.destroyed = true;
    this.previewToken++;
    this.pause();
    if (this.peekTimer) clearTimeout(this.peekTimer);
    if (this.ro) this.ro.disconnect();
    if (this.onFullChange) this.el.ownerDocument.removeEventListener("fullscreenchange", this.onFullChange);
    if (this.el.ownerDocument.fullscreenElement === this.el) this.el.ownerDocument.exitFullscreen?.();
    this.listeners.turn.clear();
    this.listeners.layout.clear();
    this.el.innerHTML = "";
    this.el.classList.remove("tb-viz", `tb-tier-${this.tier}`);
    delete this.el.dataset.tbPeek;
    delete this.el.dataset.tbNarrow;
    delete this.el.dataset.tbFull;
  }
}

// ---------------------------------------------------------------- helpers

/**
 * Where a viewer's frames come from, and the board they are drawn on.
 *
 *   replay   every frame, decoded in one pass: a stage's and a player's, and a tile's or a thumb's
 *            given nothing lighter
 *   frame    one stored frame, the last of a match: a tile's or a thumb's. It carries its own size
 *            and water, so the board is the frame's and the component is never called
 *   board    the board's turn zero, through the cartridge: a tile or a thumb for a match nobody has
 *            played yet, or one Soma holds no frame for
 *
 * A tile or a thumb takes the lightest it is given, so a card holding both its last frame and its
 * replay rests on the frame and keeps the replay for a preview.
 */
function sourceOf(tier, replay, opts) {
  const small = !FULL.has(tier);
  if (small && opts.frame) return { kind: "frame", frames: [opts.frame], map: boardOfFrame(opts.frame) };
  if (small && opts.board && !replay) return { kind: "board", frames: [openingOf(opts.board)], map: opts.board };
  if (replay) return { kind: "replay", frames: allFrames(replay), map: replay.map };
  throw new Error("nothing to draw: give a replay, a frame or a board");
}

/**
 * A frame as the board `Renderer.setBoard` reads: `{ rows, cols, water }`, the water a bare RLE.
 *
 * A frame says `size: [rows, cols]` and `water: { rle }` where a board file says `rows`, `cols`
 * and `water`, and this is the whole of the difference. The frame's hills are the standing ones
 * only, so a board made from a frame knows nothing of the razed: a tile or a thumb does not draw
 * them, and does not need to.
 */
export function boardOfFrame(frame) {
  const [rows, cols] = frame?.size ?? [0, 0];
  const water = Array.isArray(frame?.water) ? frame.water : (frame?.water?.rle ?? []);
  return { rows, cols, water };
}

/**
 * How many hills each of `n` seats starts with on a board: a board lists its hills in orbits, so
 * hill `i` is seat `i % players`'s.
 */
export function hillsPerSeat(map, n) {
  const out = new Array(n).fill(0);
  const hills = Array.isArray(map?.hills) ? map.hills : [];
  const players = Number(map?.players) || n;
  hills.forEach((_, i) => {
    const seat = i % players;
    if (seat < n) out[seat]++;
  });
  return out;
}

/**
 * The turns a tile's preview decodes for a match of `turns` turns: the last forty, or all of a
 * shorter one, and turn zero alone for a match that never started.
 */
export function previewRange(turns) {
  const to = Math.max(0, Math.floor(Number(turns) || 0));
  return [Math.max(0, to - PREVIEW_TURNS), to];
}

/** The `k` seats with the highest scores, the lower seat first on a tie. */
function leaders(score, k) {
  return score
    .map((s, seat) => ({ s, seat }))
    .sort((a, b) => b.s - a.s || a.seat - b.seat)
    .slice(0, k)
    .map((x) => x.seat);
}

function antsBySeat(f) {
  const ants = new Array(f.score.length).fill(0);
  for (const a of f.ants) ants[a[2]] = (ants[a[2]] ?? 0) + 1;
  return ants;
}

/**
 * The turns worth jumping to.
 *
 * A hill falls and a colony ends: both are visible in the frames without knowing any rule, because
 * a hill leaving the list means it was razed and an ant count reaching zero means a seat is out.
 */
function findEvents(frames, lo, hi) {
  const out = [];
  for (let i = lo + 1; i <= hi; i++) {
    const a = frames[i - 1];
    const b = frames[i];
    for (let seat = 0; seat < b.score.length; seat++) {
      const wasHills = a.hills.filter((h) => h[2] === seat).length;
      const nowHills = b.hills.filter((h) => h[2] === seat).length;
      if (nowHills < wasHills) out.push({ i, turn: b.turn, seat, kind: "razed", what: "hill razed" });
      if (count(a.ants, seat) > 0 && count(b.ants, seat) === 0) {
        out.push({ i, turn: b.turn, seat, kind: "wiped", what: "colony wiped out" });
      }
    }
  }
  return out;
}

/**
 * A frame's deaths and razings as sentences: how many ants each seat lost and to whom, and whose
 * hill fell to whom. For the canvas's label; the board draws the same record.
 */
export function describeEvents(f, names) {
  const out = [];
  const lost = new Map();
  for (const d of f.deaths ?? []) {
    const seat = d.ant[2];
    const e = lost.get(seat) ?? { n: 0, by: new Set(), collided: 0 };
    e.n++;
    if (d.by.length) for (const k of d.by) e.by.add(k[2]);
    else e.collided++;
    lost.set(seat, e);
  }
  for (const [seat, e] of [...lost].sort((a, b) => a[0] - b[0])) {
    const who = [...e.by].sort().map((s) => names[s]);
    let s = `${names[seat]} lost ${e.n} ant${e.n === 1 ? "" : "s"}`;
    if (who.length) s += ` to ${who.join(" and ")}`;
    if (e.collided) s += e.by.size ? `, ${e.collided} in a collision` : " in a collision";
    out.push(s);
  }
  for (const [, , owner, by] of f.razed ?? []) out.push(`${names[owner]}'s hill razed by ${names[by]}`);
  return out;
}

function count(ants, seat) {
  let n = 0;
  for (const a of ants) if (a[2] === seat) n++;
  return n;
}

/**
 * How many columns the seat cards lay `n` seats out in, in a viewer `width` pixels wide: every
 * seat on one row if each gets `min` pixels there and there are at most four, else the fewest rows
 * that give each seat that much -- which is also the most even split, so six seats are two rows of
 * three rather than a row of four over a row of two. Below `NARROW` it is two to a row, whatever
 * the count.
 *
 * It reads the seat count and the width and nothing else. A layout that looked at the names or the
 * numbers could change its row count in the middle of a match, and the board under it would jump.
 */
export function seatColumns(n, width, min = MIN_SEAT) {
  if (width < NARROW) return Math.min(n, 2);
  for (let rows = 1; rows < n; rows++) {
    const cols = Math.ceil(n / rows);
    if (cols <= MAX_SEAT_COLS && cols * min <= width) return cols;
  }
  return 1;
}

/**
 * What to call each seat: `{ name, by }`. What the host says, else what the replay says, else the
 * hash, else the seat number.
 *
 * The envelope names a seat as the referee knew it -- by the hash of its weights, or by a label a
 * local run chose -- and a host usually knows better: the web application knows the model's name
 * and whose it is, and showed eight characters of a hash here while every other panel on the same
 * page said "mover by @someone". `by` is the host's own words, shown as given; who owns a model is
 * the platform's business, not the cartridge's.
 */
export function seatLabels(replay, n, given) {
  const out = [];
  for (let i = 0; i < n; i++) out[i] = { name: `seat ${i}`, by: "" };
  if (Array.isArray(replay.seats)) {
    for (const s of replay.seats) {
      const i = s.seat ?? 0;
      if (!out[i]) continue;
      out[i].name = s.label ?? (s.weights_hash ? s.weights_hash.slice(7, 15) : out[i].name);
    }
  }
  if (Array.isArray(given)) {
    for (const g of given) {
      const seat = out[g?.seat];
      if (!seat) continue;
      if (g.name) seat.name = String(g.name);
      if (g.by) seat.by = String(g.by);
    }
  }
  return out;
}

/**
 * How many moves from `from` each cell is, out to `reach`, on a board that wraps; -1 beyond it.
 *
 * Water is the only wall. Food blocks a move too, but it comes and goes, and a ring that flickered
 * as food fell would be drawing the food rather than the danger. This decides nothing about the
 * match: nothing reads it but the pen that rings a hill.
 */
export function stepsFrom(water, rows, cols, from, reach) {
  const dist = new Int8Array(rows * cols).fill(-1);
  dist[from] = 0;
  let edge = [from];
  for (let d = 1; d <= reach && edge.length; d++) {
    const next = [];
    for (const i of edge) {
      const r = (i / cols) | 0;
      const c = i - r * cols;
      const around = [
        ((r + rows - 1) % rows) * cols + c,
        ((r + 1) % rows) * cols + c,
        r * cols + ((c + cols - 1) % cols),
        r * cols + ((c + 1) % cols),
      ];
      for (const j of around) {
        if (dist[j] !== -1 || water[j]) continue;
        dist[j] = d;
        next.push(j);
      }
    }
    edge = next;
  }
  return dist;
}

/**
 * The hills with an enemy ant within `reach` moves, each with how many moves the nearest one is.
 *
 * Moves, not distance as the crow flies: on a maze an enemy one wall away from a hill can be thirty
 * moves from it, and a ring that lit up for it would point at nothing. Hills never move, so each
 * one's step counts are worked out once and kept on `board.steps`.
 */
export function threatsIn(frame, board, reach) {
  const out = [];
  for (const [r, c, owner] of frame.hills) {
    const at = r * board.cols + c;
    let steps = board.steps.get(at);
    if (!steps) {
      steps = stepsFrom(board.water, board.rows, board.cols, at, reach);
      board.steps.set(at, steps);
    }
    let best = -1;
    for (const [ar, ac, ao] of frame.ants) {
      if (ao === owner) continue;
      const d = steps[ar * board.cols + ac];
      if (d >= 0 && (best < 0 || d < best)) best = d;
    }
    if (best >= 0) out.push({ r, c, owner, steps: best, reach });
  }
  return out;
}

/**
 * For each seat, the frame at which it first saw each cell, or 65535 where it never did.
 *
 * The frames are decoded from turn zero, so the first one's `discovered` is the opening vision and
 * every cell a seat ever knows is announced exactly once after that.
 */
function firstSeen(frames, cols, cells) {
  const at = frames[0].score.map(() => new Uint16Array(cells).fill(0xffff));
  frames.forEach((f, i) => {
    (f.discovered ?? []).forEach((list, seat) => {
      const mine = at[seat];
      if (!mine) return;
      for (const [r, c] of list) {
        const k = r * cols + c;
        if (mine[k] === 0xffff) mine[k] = i;
      }
    });
  });
  return at;
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function esc(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}
