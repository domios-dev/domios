import { isIP } from "node:net";

export function clientIp(forwarded: string | undefined, remote: string | undefined, emulator: boolean): string {
  const hops = forwarded?.split(",").map((value) => value.trim()) ?? [];
  // Direct Google ingress appends the client and load balancer after any supplied prefix.
  const candidate = emulator ? (remote ?? "127.0.0.1") : hops.at(-2);
  if (!candidate || !isIP(candidate)) throw new Error("Missing trusted client address");
  const normalized = candidate.toLowerCase().replace(/^::ffff:(\d+\.\d+\.\d+\.\d+)$/, "$1");
  return isIP(normalized) === 6 ? new URL(`http://[${normalized}]`).hostname : normalized;
}
