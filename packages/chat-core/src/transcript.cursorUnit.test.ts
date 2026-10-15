import assert from "node:assert/strict";
import { createTranscriptState, transcriptOffsetKind, type TranscriptState } from "./transcript";

const state = (unit?: "bytes" | "items"): TranscriptState => ({
  ...createTranscriptState(),
  segments:[{kind:"snapshot",range:{start:10,end:20,unit},rows:[]}, {kind:"delta",range:null,rows:[]}],
});
assert.equal(transcriptOffsetKind(createTranscriptState()), "bytes");
assert.equal(transcriptOffsetKind(state()), "bytes");
assert.equal(transcriptOffsetKind(state("bytes")), "bytes");
assert.equal(transcriptOffsetKind(state("items")), "items");
console.log("PASS backfill cursor units survive unpositioned live deltas");
