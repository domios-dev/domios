import { useEffect, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { SettingsSection, SwitchTile } from "./shared";
import {
  desktopSurfaceReadsOn,
  establishNotifyPermission,
  readNotifyPermission,
  type NotifyPermission,
} from "../../lib/notifyPermission";
import { establishChimeContext } from "../../lib/notificationSound";

/**
 * What a notification is allowed to reach at all: the indicator, the system
 * toast, and the speakers.
 *
 * The two that need the system's consent establish it from the enabling
 * gesture rather than from the first send — the OS prompt belongs to the click
 * that asked for the surface, and an audio context only starts running inside
 * one. A refusal therefore leaves the switch reading off instead of promising a
 * toast that never comes, and the refusal is explained here, beside the switch
 * it is about. Enabling here is one such gesture, never the only one: a sound
 * already on in stored config is established by the first gesture anywhere.
 *
 * The sound switch is the MASTER MUTE, not the whole answer: which levels chime
 * is each level's own card. It is retained rather than retired because muting
 * everything must not destroy those answers, and because it is the one moment
 * the browser's audio permission has a gesture to hang off.
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
    <SettingsSection title="Surfaces">
      <SwitchTile
        label="Show the bell"
        hint="Off hides the indicator entirely. History is still kept, and the panel stays the complete log."
        checked={config.notifications.show_bell !== false}
        onChange={(v) => onUpdate("notifications", { show_bell: v })}
      />
      <SwitchTile
        label="Desktop notifications"
        hint="System toasts. Permission is asked for when you switch this on."
        checked={desktopSurfaceReadsOn(config.notifications.desktop, permission)}
        onChange={(next) => void switchDesktop(next)}
      />
      {permission === "denied" && (
        <p
          data-testid="settings-desktop-permission-refused"
          className="text-[10px] text-[var(--ct-yellow)] leading-relaxed"
        >
          The system refused notifications for CodeTerm, so this stays off until you allow them in
          system settings.
        </p>
      )}
      <SwitchTile
        label="Notification sounds"
        hint="Master mute. Each level below still decides for itself."
        checked={config.notifications.sound}
        onChange={(v) => {
          if (v) establishChimeContext();
          onUpdate("notifications", { sound: v });
        }}
      />
    </SettingsSection>
  );
}
