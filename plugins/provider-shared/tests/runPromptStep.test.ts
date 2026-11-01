import { describe, expect, it } from "vitest";
import type { BlueprintStepRequest, HostAgentRun, HostAgentRunReply } from "@codeterm/plugin-sdk";
import { runPromptDescriptor, runPromptSteps } from "../runPromptStep";

function request(params: Record<string, unknown> = {}): BlueprintStepRequest {
  return { context: { scope: { workspace: "w", team: null, run: "r" }, owner: "o", node: "n", attempt: "a", operation: "op", nodeLabel: "n", inputLineage: [] }, nodeType: "fixture.run_prompt", revision: 1, params: { prompt: "opaque prompt", cwd: "/work", ...params }, inputs: {}, job: "own-run", deadlineMs: 1000 };
}
function fixture(reply: HostAgentRunReply = { state: "done", text: "opaque answer" }) {
  const starts: unknown[] = [];
  const waits: string[] = [];
  const cancels: string[] = [];
  const host: HostAgentRun = {
    start(opts, then) { starts.push(opts); return then({ run: "new-run" }); },
    await(run, then) { waits.push(run); return then(reply); },
    cancel(run, then) { cancels.push(run); return then({ state: "canceled" }); },
  };
  return { steps: runPromptSteps(() => host), starts, waits, cancels };
}
describe("provider Blueprint run prompt lifecycle", () => {
  it("declares identical ports and lifecycle for each provider", () => {
    for (const id of ["claude", "codex", "opencode"]) {
      const descriptor = runPromptDescriptor(id, id);
      expect(descriptor.id).toBe(`${id}.run_prompt`);
      expect(descriptor.outcomes).toEqual(["done", "failed"]);
      expect(descriptor.lifecycle).toEqual({ reconcile: true, cancel: true });
      expect(descriptor.outputs).toEqual({ text: { kind: "text", minItems: 1, maxItems: 1 } });
    }
  });
  it("submits once, awaits the same run and cancels it", () => {
    const f = fixture();
    expect(f.steps.blueprintSubmit(request())).toEqual({ job: "new-run", state: "running", outcome: null, outputs: {} });
    expect(f.steps.blueprintReconcile(request()).outputs.text).toEqual([{ kind: "text", text: "opaque answer" }]);
    expect(f.steps.blueprintCancel(request()).state).toBe("canceled");
    expect(f.starts).toEqual([{ prompt: "opaque prompt", cwd: "/work", timeoutMs: 600000 }]);
    expect(f.waits).toEqual(["own-run"]); expect(f.cancels).toEqual(["own-run"]);
  });
  it("fences bound context without letting its fences close the block", () => {
    const f = fixture(); const req = request(); req.inputs.context = [{ kind: "text", text: "```\nopaque page" }];
    f.steps.blueprintSubmit(req);
    expect(f.starts).toEqual([{ prompt: "opaque prompt\n\nContext:\n````text\n```\nopaque page\n````", cwd: "/work", timeoutMs: 600000 }]);
  });
  it("rejects invalid params and oversized combined inputs before start", () => {
    const f = fixture();
    for (const params of [{ prompt: "" }, { prompt: "a".repeat(32769) }, { cwd: "relative" }, { cwd: 1 }, { timeout_ms: 3600001 }, { timeout_ms: 1.2 }, { model: 3 }]) {
      expect(f.steps.blueprintSubmit(request(params)).outcome).toBe("failed");
    }
    const req = request(); req.inputs.context = [{kind:"text",text:"x".repeat(50000)}];
    expect(f.steps.blueprintSubmit(req).outcome).toBe("failed");
    expect(f.starts).toEqual([]);
  });
  it("never resubmits an unknown run after restart", () => {
    const f = fixture({state:"unknown"});
    expect(f.steps.blueprintReconcile(request())).toEqual({job:"own-run",state:"unknown",outcome:null,outputs:{}});
    expect(f.starts).toEqual([]);
  });
  it("maps provider failures and empty answers to failed without leaking diagnostics", () => {
    for (const reply of [{state:"failed",reason:"credential"}, {state:"done",text:""}, {error:"credential"}]) {
      const update = fixture(reply as HostAgentRunReply).steps.blueprintReconcile(request());
      expect(update.outcome).toBe("failed"); expect(JSON.stringify(update)).not.toContain("credential");
    }
  });
  it("bounds escaped and multibyte outputs as serialized step payload", () => {
    const update = fixture({state:"done",text:'\u0001😀"'.repeat(20000)}).steps.blueprintReconcile(request());
    expect(Buffer.byteLength(JSON.stringify(update))).toBeLessThan(65536);
    expect((update.outputs.text[0] as {text:string}).text.endsWith("[truncated]")).toBe(true);
  });
});
