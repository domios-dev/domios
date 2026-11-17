import { createContext } from "react";

/** The camera scale while the canvas rests, or `null` while a zoom gesture or fly changes it. */
export type CameraRest = number | null;
export type CameraRestEvent = { kind: "paint" | "settle"; scale: number };

/** Below this, frame text is too small to read and viewport units drift too far from the plain frame. */
export const SHARP_FRAME_MIN_SCALE = 0.5;

/** A paint at another scale starts a gesture; a pan keeps the rest; a committed transform rests. */
export function nextCameraRest(rest: CameraRest, event: CameraRestEvent): CameraRest {
  if (event.kind === "settle") return event.scale;
  return rest === event.scale ? rest : null;
}

/** Content zoom for a live frame counter-scaled to one screen pixel per pixel; 1 keeps the plain frame. */
export function sharpFrameZoom(rest: CameraRest): number {
  return rest !== null && rest >= SHARP_FRAME_MIN_SCALE && rest < 1 ? rest : 1;
}

export const CameraRestContext = createContext<CameraRest>(null);
