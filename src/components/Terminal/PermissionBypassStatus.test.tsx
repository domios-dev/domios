import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { PermissionBypassStatus } from "./PermissionBypassStatus";

describe("PermissionBypassStatus", () => {
  it("exposes the active tab sandbox state accessibly", () => {
    const sandboxed = renderToStaticMarkup(<PermissionBypassStatus autoApprove={false} />);
    const bypassed = renderToStaticMarkup(<PermissionBypassStatus autoApprove />);

    expect(sandboxed).toContain('role="status"');
    expect(sandboxed).toContain("Provider permissions and sandbox active for this session");
    expect(sandboxed).toContain("Sandbox active");
    expect(bypassed).toContain("Provider permission bypass active for this session");
    expect(bypassed).toContain("Permission bypass");
  });
});

finish();
