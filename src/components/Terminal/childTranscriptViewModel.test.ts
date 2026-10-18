import { describe, expect, it } from "vitest";
import { childTranscriptViewState, childTranscriptTitle, compactSubagents, subagentTone } from "./childTranscriptViewModel";

describe("child transcript view", () => {
  it("projects initial loading, empty, error and ready states", () => {
    expect(childTranscriptViewState({loading:true,error:null,itemCount:0})).toBe("loading");
    expect(childTranscriptViewState({loading:false,error:null,itemCount:0})).toBe("empty");
    expect(childTranscriptViewState({loading:false,error:"failed",itemCount:0})).toBe("error");
    expect(childTranscriptViewState({loading:true,error:null,itemCount:2})).toBe("ready");
    expect(childTranscriptViewState({loading:false,error:null,itemCount:2})).toBe("ready");
    expect(childTranscriptTitle(null)).toBe("Subagent");
  });
  it("limits capsules and makes overflow available", () => {
    expect(compactSubagents([1,2,3,4,5],false)).toEqual({visible:[1,2,3],hidden:2});
    expect(compactSubagents([1,2,3,4],true)).toEqual({visible:[1,2,3,4],hidden:0});
    expect(compactSubagents([],false)).toEqual({visible:[],hidden:0});
    expect(subagentTone("running")).toBe("active");
    expect(subagentTone("unknown")).toBe("scheduled");
  });
});
