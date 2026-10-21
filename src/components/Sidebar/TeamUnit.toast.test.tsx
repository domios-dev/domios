import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification, ProviderDescriptor } from "../../lib/types";
import type { TeamMemberRole } from "../../lib/teamActions";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { TeamUnit } from "./TeamUnit";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness({ onAddMember }: { onAddMember: (role: TeamMemberRole, providerId: string, model?: string) => Promise<void> }) {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  const provider = {
    id: "fixture-provider", display_name: "Fixture", transport: "pty", behavior_config: { model: "none" },
  } as ProviderDescriptor;
  return <MultiplexerActionsContext.Provider value={actions}>
    <TeamUnit
      team={{ id: "team-a", name: "Team A", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
      plan={null} tabs={{}} managers={null} managerCount={0} workers={null} workerCount={0}
      workingWorkerCount={0} agents={null} providers={[provider]} addableRoles={["worker"]} onAddMember={onAddMember}
      onOpenEffort={() => {}}
    />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

it("shows floating Team action failures in a retryable toast", async () => {
  const onAddMember = vi.fn(async (_role: TeamMemberRole, _providerId: string, _model?: string) => {
    throw new Error("opaque add member failure");
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness onAddMember={onAddMember} />));
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Team actions"]')!.click());
    const menu = document.querySelector<HTMLElement>('[data-testid="team-menu"]')!;
    const addWorker = Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
      .find((button) => button.textContent?.includes("Add worker"));
    await act(async () => addWorker!.click());
    const provider = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
      .find((button) => button.textContent?.includes("Fixture"));
    await act(async () => provider!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(onAddMember).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});
