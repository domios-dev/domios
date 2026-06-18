import { useEffect, useState } from "react";
import { listen } from "./bridge";
import type { Notification, PluginActivity } from "./types";
import { useMultiplexer } from "../store/multiplexer";

type PluginActivityStatus = PluginActivity["status"];

const STATUSES = new Set<PluginActivityStatus>(["running", "done", "error"]);

export function normalizePluginActivityEvent(payload: unknown): PluginActivity | null {
  if (!payload || typeof payload !== "object") return null;
  const raw = payload as Record<string, unknown>;
  if (
    typeof raw.id !== "string" ||
    typeof raw.pluginId !== "string" ||
    typeof raw.kind !== "string" ||
    typeof raw.label !== "string" ||
    typeof raw.status !== "string" ||
    !STATUSES.has(raw.status as PluginActivityStatus) ||
    typeof raw.startedAt !== "number" ||
    typeof raw.updatedAt !== "number"
  ) {
    return null;
  }

  return {
    id: raw.id,
    pluginId: raw.pluginId,
    kind: raw.kind,
    label: raw.label,
    percent: typeof raw.percent === "number" ? raw.percent : null,
    status: raw.status as PluginActivityStatus,
    error: typeof raw.error === "string" ? raw.error : null,
    startedAt: raw.startedAt,
    updatedAt: raw.updatedAt,
  };
}

export function reducePluginActivity(
  activities: Map<string, PluginActivity>,
  payload: unknown,
): Map<string, PluginActivity> {
  const activity = normalizePluginActivityEvent(payload);
  if (!activity) return activities;

  const next = new Map(activities);
  if (activity.status === "running") {
    next.set(activity.pluginId, activity);
  } else {
    next.delete(activity.pluginId);
  }
  return next;
}

export function usePluginActivity(): { activities: Map<string, PluginActivity> } {
  const [activities, setActivities] = useState<Map<string, PluginActivity>>(() => new Map());
  const { upsertToast, dismissToast } = useMultiplexer();

  useEffect(() => {
    let mounted = true;
    let cleanup: (() => void) | null = null;

    listen<unknown>("plugin-activity", ({ payload }) => {
      const activity = normalizePluginActivityEvent(payload);
      if (!activity || !mounted) return;

      setActivities((current) => reducePluginActivity(current, activity));
      if (activity.status === "running") {
        upsertToast(pluginActivityToast(activity));
      } else {
        dismissToast(pluginActivityToastId(activity.pluginId));
      }
    })
      .then((unlisten) => {
        cleanup = unlisten;
        if (!mounted) unlisten();
      })
      .catch((e) => console.warn("plugin-activity listen failed:", e));

    return () => {
      mounted = false;
      cleanup?.();
    };
  }, [dismissToast, upsertToast]);

  return { activities };
}

function pluginActivityToast(activity: PluginActivity): Omit<Notification, "timestamp" | "read"> {
  return {
    id: pluginActivityToastId(activity.pluginId),
    key: pluginActivityToastId(activity.pluginId),
    pane_id: null,
    workspace_id: null,
    title: "Plugin activity",
    body: activity.label,
    level: "Info",
    sticky: true,
  };
}

function pluginActivityToastId(pluginId: string): string {
  return `plugin-activity:${pluginId}`;
}
