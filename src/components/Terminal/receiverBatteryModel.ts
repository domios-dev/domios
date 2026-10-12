export interface ReceiverBattery {
  slot: number;
  connected: boolean;
  percent: number | null;
}

export function connectedReceiverBatteries(value: unknown): ReceiverBattery[] {
  if (!Array.isArray(value) || value.length > 2) return [];
  const slots = new Set<number>();
  const result: ReceiverBattery[] = [];
  for (const item of value) {
    if (!item || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    if ((row.slot !== 1 && row.slot !== 2) || slots.has(row.slot) || typeof row.connected !== "boolean") return [];
    slots.add(row.slot);
    if (!row.connected) continue;
    if (typeof row.percent !== "number" || !Number.isInteger(row.percent) || row.percent < 0 || row.percent > 100) return [];
    result.push({ slot: row.slot, connected: true, percent: row.percent });
  }
  return result;
}
