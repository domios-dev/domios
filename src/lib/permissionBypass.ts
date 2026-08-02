export interface PermissionBypassPreference {
  spawn_skip_permissions?: boolean;
  spawn_skip_permissions_remembered?: boolean;
}

export function rememberedPermissionBypass(
  preference: PermissionBypassPreference | undefined,
): boolean {
  return preference?.spawn_skip_permissions_remembered === true
    && preference.spawn_skip_permissions === true;
}

export function durablePermissionBypassPreference(
  selected: boolean,
  remember: boolean,
): Required<PermissionBypassPreference> {
  const enabled = selected && remember;
  return {
    spawn_skip_permissions: enabled,
    spawn_skip_permissions_remembered: enabled,
  };
}
