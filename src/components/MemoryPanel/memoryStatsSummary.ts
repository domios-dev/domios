export function memoryStatsSummary(memories: number, connections: number, indexed: number): string {
  return `${memories.toLocaleString("en-US")} ${memories === 1 ? "memory" : "memories"} · ${connections.toLocaleString("en-US")} ${connections === 1 ? "connection" : "connections"} · ${indexed.toLocaleString("en-US")} indexed`;
}
