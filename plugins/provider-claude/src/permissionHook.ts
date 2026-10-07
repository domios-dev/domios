export function permissionHookCapability() {
  return {
    protocol: "claude-permission-request",
    options: [
      { label: "Allow", value: "allow" },
      { label: "Deny", value: "deny" },
    ],
  };
}
