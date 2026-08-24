import { useEffect, useRef, useState } from "react";
import { invoke, listen } from "./bridge";
import type { Notification, PluginActivity } from "./types";
import { useMultiplexer } from "../store/multiplexer";

type PluginActivityStatus = PluginActivity["status"];

const STATUSES = new Set<PluginActivityStatus>(["running", "done", "error"]);

export interface PluginActivityDisplay {
  displayName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
}

export type PluginActivityDisplayMap = Map<string, PluginActivityDisplay>;

interface SnapshotPlugin {
  id?: string | null;
  display_name?: string | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
}

interface PluginSnapshot {
  plugins?: SnapshotPlugin[] | null;
  marketplace?: SnapshotPlugin[] | null;
}

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

export function buildPluginActivityDisplayMap(snapshot: PluginSnapshot | null | undefined): PluginActivityDisplayMap {
  const map: PluginActivityDisplayMap = new Map();
  const add = (plugin: SnapshotPlugin) => {
    if (!plugin.id) return;
    map.set(plugin.id, {
      displayName: plugin.display_name || plugin.id,
      iconSvg: plugin.icon_svg ?? null,
      iconColor: plugin.icon_color ?? null,
      iconHint: plugin.icon_hint ?? null,
    });
  };

  for (const plugin of snapshot?.marketplace ?? []) add(plugin);
  for (const plugin of snapshot?.plugins ?? []) add(plugin);
  return map;
}

export function resolvePluginActivityDisplay(
  pluginId: string,
  displayMap: PluginActivityDisplayMap,
): PluginActivityDisplay {
  return displayMap.get(pluginId) ?? { displayName: pluginId };
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

export function usePluginActivity(): {
  activities: Map<string, PluginActivity>;
  displayMap: PluginActivityDisplayMap;
} {
  const [activities, setActivities] = useState<Map<string, PluginActivity>>(() => new Map());
  const [displayMap, setDisplayMap] = useState<PluginActivityDisplayMap>(() => new Map());
  const activitiesRef = useRef(activities);
  const displayMapRef = useRef(displayMap);
  const { upsertToast, dismissToast } = useMultiplexer();

  useEffect(() => {
    let mounted = true;
    invoke<PluginSnapshot>("plugin_manager_snapshot")
      .then((snapshot) => {
        if (!mounted) return;
        const nextDisplayMap = buildPluginActivityDisplayMap(snapshot);
        displayMapRef.current = nextDisplayMap;
        setDisplayMap(nextDisplayMap);
        for (const activity of activitiesRef.current.values()) {
          upsertToast(pluginActivityToast(activity, nextDisplayMap));
        }
      })
      .catch((e) => console.warn("plugin activity display lookup failed:", e));
    return () => {
      mounted = false;
    };
  }, [upsertToast]);

  useEffect(() => {
    let mounted = true;
    let cleanup: (() => void) | null = null;

    listen<unknown>("plugin-activity", ({ payload }) => {
      const activity = normalizePluginActivityEvent(payload);
      if (!activity || !mounted) return;

      setActivities((current) => {
        const next = reducePluginActivity(current, activity);
        activitiesRef.current = next;
        return next;
      });
      if (activity.status === "running") {
        upsertToast(pluginActivityToast(activity, displayMapRef.current));
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

  return { activities, displayMap };
}

function pluginActivityToast(
  activity: PluginActivity,
  displayMap: PluginActivityDisplayMap,
): Omit<Notification, "timestamp" | "read"> {
  const display = resolvePluginActivityDisplay(activity.pluginId, displayMap);
  return {
    id: pluginActivityToastId(activity.pluginId),
    key: pluginActivityToastId(activity.pluginId),
    tab_id: null,
    workspace_id: null,
    title: display.displayName,
    body: activity.label,
    icon_svg: display.iconSvg ?? null,
    icon_color: display.iconColor ?? null,
    icon_hint: display.iconHint ?? null,
    severity: "Info",
    sticky: true,
  };
}

function pluginActivityToastId(pluginId: string): string {
  return `plugin-activity:${pluginId}`;
}
