import { describe, expect, it } from "vitest";
import { TransportError } from "../transportClient";
import { createFrontendUpdateClient } from "./client";
import { frontendCheckpointClientId, frontendUpdateDiagnostic, FrontendUpdateError } from "./bootstrapModel";

describe("optional frontend update bootstrap", () => {
  it("converts a resize owner identity to a stable filename-safe checkpoint scope", async () => {
    const identity = "client:550e8400-e29b-41d4-a716-446655440000";
    const clientId = frontendCheckpointClientId(identity);
    expect(clientId).toBe("client-550e8400-e29b-41d4-a716-446655440000");
    const calls: string[] = [];
    await createFrontendUpdateClient({ json: async <T>(_method: string, path: string) => { calls.push(path); return null as T; } }, clientId).checkpoint();
    expect(calls).toEqual([`/api/frontend/clients/${clientId}/checkpoint`]);
    expect(frontendCheckpointClientId("restored-client")).toBe("restored-client");
    for (const invalid of ["../other", "a/b", "client:", "x".repeat(129)]) {
      expect(() => frontendCheckpointClientId(invalid)).toThrow(FrontendUpdateError);
    }
  });
  it("classifies failures without exposing URLs, credentials, or response contents", () => {
    const secret = "opaque-fixture-token";
    const cause = new TransportError(404, `/api/frontend/clients/${secret}/checkpoint`, secret);
    const diagnostic = frontendUpdateDiagnostic(new FrontendUpdateError("checkpoint-unavailable", cause));
    expect(diagnostic).toEqual({ name: "FrontendUpdateError", reason: "checkpoint-unavailable", httpStatus: 404 });
    expect(JSON.stringify(diagnostic)).not.toContain(secret);
    expect(frontendUpdateDiagnostic(new Error(secret))).toEqual({ name: "Error", reason: "unexpected" });
    const renamed = new Error(secret); renamed.name = secret;
    expect(JSON.stringify(frontendUpdateDiagnostic(renamed))).not.toContain(secret);
    expect(frontendUpdateDiagnostic(new SyntaxError(secret)).reason).toBe("boot-context-invalid");
  });
});
