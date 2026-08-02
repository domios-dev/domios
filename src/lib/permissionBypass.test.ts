import { describe, expect, finish, it } from "../test/standaloneTest";
import {
  durablePermissionBypassPreference,
  rememberedPermissionBypass,
} from "./permissionBypass";

describe("permission bypass preference", () => {
  it("preselects bypass only when the selected bypass was remembered", () => {
    expect(rememberedPermissionBypass({
      spawn_skip_permissions: true,
      spawn_skip_permissions_remembered: false,
    })).toBe(false);
    expect(rememberedPermissionBypass({
      spawn_skip_permissions: true,
      spawn_skip_permissions_remembered: true,
    })).toBe(true);
  });

  it("clears both durable fields unless bypass is selected and remembered", () => {
    expect(durablePermissionBypassPreference(true, false)).toEqual({
      spawn_skip_permissions: false,
      spawn_skip_permissions_remembered: false,
    });
    expect(durablePermissionBypassPreference(false, true)).toEqual({
      spawn_skip_permissions: false,
      spawn_skip_permissions_remembered: false,
    });
    expect(durablePermissionBypassPreference(true, true)).toEqual({
      spawn_skip_permissions: true,
      spawn_skip_permissions_remembered: true,
    });
  });
});

finish();
