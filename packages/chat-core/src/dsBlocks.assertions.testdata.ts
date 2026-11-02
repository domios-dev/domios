import type { DsSegment } from "./dsBlocks";
export const committed = (segments: readonly DsSegment[]) => segments.filter((part) => part.type === "block" || part.type === "diagram");
export const blocks = (segments: readonly DsSegment[]) => segments.flatMap((part) => part.type === "block" ? [{ block: part.block, start: part.start, end: part.end }] : []);
export const diagrams = (segments: readonly DsSegment[]) => segments.flatMap((part) => part.type === "diagram" ? [{ text: part.text, start: part.start, end: part.end }] : []);
