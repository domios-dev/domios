export type BackgroundProcessAuthority = Pick<import("../../gen/BackgroundProcessSnapshot").BackgroundProcessSnapshot, "rootThreadId" | "bindingNonce" | "epoch">;

export type BackgroundProcessEntry = import("../../gen/BackgroundProcessEntry").BackgroundProcessEntry;

export type BackgroundProcessSnapshot = import("../../gen/BackgroundProcessSnapshot").BackgroundProcessSnapshot;

export type BackgroundStopResult =
  | { status: "stopped"; snapshot: BackgroundProcessSnapshot }
  | { status: "degraded"; reason: "attributionUnavailable" | "exitUnconfirmed" };

export function backgroundProcessesSuffix(authority: BackgroundProcessAuthority): string {
  const query = new URLSearchParams({
    rootThreadId: authority.rootThreadId,
    bindingNonce: authority.bindingNonce,
    epoch: String(authority.epoch),
  });
  return `/agent/runtime/background-processes?${query}`;
}
