import { useEffect, useSyncExternalStore } from "react";
import { invoke, listen } from "../../lib/bridge";
import { nativeBridge } from "../../lib/nativeBridge";
import { connectedReceiverBatteries, type ReceiverBattery } from "./receiverBatteryModel";

interface BatteryEntry {
  value: ReceiverBattery[];
  subscribers: Set<() => void>;
  pending?: Promise<void>;
  revision: number;
}
const entries = new Map<string, BatteryEntry>();
function entryFor(pluginId: string): BatteryEntry {
  let entry = entries.get(pluginId);
  if (!entry) {
    entry = { value: [], subscribers: new Set(), revision: 0 };
    entries.set(pluginId, entry);
  }
  return entry;
}
function publish(entry: BatteryEntry, value: ReceiverBattery[]) {
  entry.value = value;
  entry.subscribers.forEach(notify => notify());
}
function refresh(pluginId: string, entry: BatteryEntry) {
  if (!nativeBridge.localRecording || entry.pending) return;
  const revision = entry.revision;
  entry.pending = invoke<unknown>("local_recording_battery", { pluginId })
    .then(value => { if (revision === entry.revision) publish(entry, connectedReceiverBatteries(value)); })
    .catch(() => { if (revision === entry.revision) publish(entry, []); })
    .finally(() => { entry.pending = undefined; });
}

export function useReceiverBattery(pluginId: string): ReceiverBattery[] {
  const entry = entryFor(pluginId);
  const batteries = useSyncExternalStore(notify => {
    entry.subscribers.add(notify);
    return () => { entry.subscribers.delete(notify); };
  }, () => entry.value);
  useEffect(() => {
    if (!nativeBridge.localRecording) return;
    let disposed = false;
    const unlisten: Array<() => void> = [];
    const update = () => refresh(pluginId, entry);
    const invalidate = () => { entry.revision += 1; publish(entry, []); };
    const onVisible = () => { if (!document.hidden) update(); };
    window.addEventListener("focus", update);
    document.addEventListener("visibilitychange", onVisible);
    void Promise.all([
      listen<{ pluginId: string; available: boolean; enabled: boolean }>("local-recording-status", ({ payload }) => {
        if (payload.pluginId !== pluginId) return;
        if (!payload.available || !payload.enabled) invalidate();
        else update();
      }),
      listen<{ pluginId: string }>("local-recording-capture-invalidated", ({ payload }) => {
        if (payload.pluginId === pluginId) invalidate();
      }),
      listen("local-recording-capture", update),
    ]).then(listeners => {
      if (disposed) listeners.forEach(stop => stop());
      else { unlisten.push(...listeners); update(); }
    }).catch(() => {});
    return () => {
      disposed = true;
      window.removeEventListener("focus", update);
      document.removeEventListener("visibilitychange", onVisible);
      unlisten.forEach(stop => stop());
      entry.revision += 1;
      if (!entry.subscribers.size) publish(entry, []);
    };
  }, [pluginId, entry]);
  return batteries;
}
