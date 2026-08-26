import { useEffect, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { Toggle } from "./shared";
import {
  desktopSurfaceReadsOn,
  establishNotifyPermission,
  readNotifyPermission,
  type NotifyPermission,
} from "../../lib/notifyPermission";
import { establishChimeContext } from "../../lib/notificationSound";

/**
 * The two presentation surfaces that need the system's consent before they can
 * deliver anything: the desktop toast and the chime.
 *
 * Both establish that consent from the enabling gesture rather than from the
 * first send — the OS prompt belongs to the click that asked for the surface,
 * and an audio context only starts running inside one. A refusal therefore
 * leaves the switch reading off instead of promising a toast that never comes.
 * Enabling here is one such gesture, never the only one: a sound already on in
 * stored config is established by the first gesture anywhere in the app.
 */
export function NotificationSurfaces({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const [permission, setPermission] = useState<NotifyPermission>("unknown");

  useEffect(() => {
    let live = true;
    void readNotifyPermission().then((state) => {
      if (live) setPermission(state);
    });
    return () => {
      live = false;
    };
  }, []);

  const switchDesktop = async (next: boolean) => {
    const state = await establishNotifyPermission(next);
    setPermission(state);
    if (next && state === "denied") return;
    onUpdate("notifications", { desktop: next });
  };

  return (
    <>
      <Toggle
        label="Desktop notifications"
        checked={desktopSurfaceReadsOn(config.notifications.desktop, permission)}
        onChange={(next) => void switchDesktop(next)}
      />
      {permission === "denied" && (
        <p
          data-testid="settings-desktop-permission-refused"
          className="text-[10px] text-[var(--ct-warning,#d29922)] leading-relaxed"
        >
          The system refused notifications for CodeTerm, so this stays off until you allow them in
          system settings.
        </p>
      )}
      <Toggle
        label="Notification sounds"
        checked={config.notifications.sound}
        onChange={(v) => {
          if (v) establishChimeContext();
          onUpdate("notifications", { sound: v });
        }}
      />
    </>
  );
}
