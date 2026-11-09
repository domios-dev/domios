import type {ViewHostAction, ViewInputMediaType} from "./capabilities";

export type ViewHostRefusalCode = "denied" | "unavailable" | "invalid";
export type ViewHostRefusal = {ok: false; code: ViewHostRefusalCode; reason: string};
export type PickedInput = Pick<import("./gen/IngestedInput").IngestedInput, "handle" | "fileName" | "contentType" | "size">;
export type PickFileResult = {ok: true; inputs: PickedInput[]; skipped: {fileName: string; code: ViewHostRefusalCode; reason: string}[]} | ViewHostRefusal;
export type ViewHostResult = {ok: true} | ViewHostRefusal;
export type ViewHostAvailability = Record<ViewHostAction, boolean>;
export type PickFileOptions = {accept?: ViewInputMediaType[]; multiple?: boolean};
