/** The capture clock: capture.mjs pauses the page clock here, so fixture times never drift between frames. */
export const FILM_NOW = Date.UTC(2026, 9, 23, 10, 0, 0);
export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;

export const ago = (minutes: number) => FILM_NOW - minutes * MINUTE;
export const ahead = (minutes: number) => FILM_NOW + minutes * MINUTE;

// Stills taken without capture.mjs's paused clock read the same instant, so "next in 13m" never turns into "overdue".
const WallDate = Date;
class FilmDate extends WallDate {
  constructor(...args: ConstructorParameters<DateConstructor> | []) {
    if (args.length) super(...(args as [number]));
    else super(FILM_NOW);
  }
  static override now() {
    return FILM_NOW;
  }
}
globalThis.Date = FilmDate as DateConstructor;
