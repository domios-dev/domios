import { describe, expect, it } from "vitest";
import { childTranscriptViewState, childTranscriptTitle, subagentTone } from "./childTranscriptViewModel";

describe("child transcript view", () => {
  it("projects initial loading, empty, error and ready states", () => {
    expect(childTranscriptViewState({loading:true,error:null,itemCount:0})).toBe("loading");
    expect(childTranscriptViewState({loading:false,error:null,itemCount:0})).toBe("empty");
    expect(childTranscriptViewState({loading:false,error:"failed",itemCount:0})).toBe("error");
    expect(childTranscriptViewState({loading:true,error:null,itemCount:2})).toBe("ready");
    expect(childTranscriptViewState({loading:false,error:null,itemCount:2})).toBe("ready");
    expect(childTranscriptTitle(null)).toBe("Subagent");
  });
  it("projects subagent tones", () => {
    expect(subagentTone("running")).toBe("active");
    expect(subagentTone("unknown")).toBe("scheduled");
  });
});
