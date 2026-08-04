import { describe, expect, finish, it } from "../test/standaloneTest";
import { remoteOutputSinceCursor } from "./outputStream";

describe("remote output resume cursor", () => {
  it("resumes the same tab from its last delivered offset without loss", () => {
    const encoded = remoteOutputSinceCursor("tab-a", "boot-a", 417);
    const padded = encoded.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(encoded.length / 4) * 4, "=");
    expect(JSON.parse(atob(padded))).toEqual({ boot: "boot-a", tabs: { "tab-a": 417 } });
  });
});

finish();
