// The same viewer, as a React component.
//
// React is a peer: this module is only ever imported by an application that already has one, and
// bundling a copy would make the cartridge depend on a React version it has no business pinning.
// The web application imports this; the book and the CLI import `index.js` and call `mount`.
//
// It is deliberately the only file here that mentions React at all.

import { createElement, useEffect, useRef } from "react";
import { Viewer } from "./shell.js";

/**
 * @param {object} props
 * @param {object} props.replay   the envelope -- it carries its own board
 * @param {number} [props.turn]   the turn to open on
 * @param {number} [props.from]   narrow the timeline without renumbering it
 * @param {number} [props.to]
 * @param {boolean} [props.autoplay]
 * @param {number} [props.speed]
 * @param {"light"|"dark"} [props.theme]   overrides the page; omit it and the viewer follows the
 *                                         page's own tokens and theme switch
 * @param {"hover"|"always"} [props.chrome] whether the tray of readouts is pinned open
 * @param {{seat: number, name?: string, by?: string}[]} [props.labels]  what to call each seat
 * @param {boolean} [props.explored]  open with each seat's explored territory drawn
 * @param {(frame: object) => void} [props.onTurn]
 */
export function AntsReplay({
  replay, turn, from, to, autoplay, speed, theme, chrome, labels, explored, onTurn, style, className,
}) {
  const host = useRef(null);
  const cb = useRef(onTurn);
  cb.current = onTurn;
  // Compared by content, so a caller building the list inline does not rebuild the viewer either.
  const said = labels ? JSON.stringify(labels) : "";

  useEffect(() => {
    if (!host.current || !replay) return;
    // The callback goes through a ref so a caller passing an inline arrow does not rebuild the
    // viewer -- and rebuilding it means decoding the whole match again.
    const v = new Viewer(host.current, replay, {
      turn, from, to, autoplay, speed, theme, chrome, explored,
      labels: said ? JSON.parse(said) : undefined,
      onTurn: (f) => cb.current && cb.current(f),
    });
    return () => v.destroy();
  }, [replay, turn, from, to, autoplay, speed, theme, chrome, explored, said]);

  // `createElement` rather than JSX, so this file is plain ES the browser and any bundler both
  // accept and the cartridge needs no JSX toolchain of its own.
  return createElement("div", { ref: host, className, style });
}

export default AntsReplay;
