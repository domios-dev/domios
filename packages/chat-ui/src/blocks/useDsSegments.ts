import { DsBlockScanner, type DsAuthority, type DsSegment } from "@codeterm/chat-core";
import { useLayoutEffect, useRef } from "react";

interface Projection { scanner: DsBlockScanner; authority: DsAuthority; generation: number }
export function useDsSegments(text: string, authority: DsAuthority, streaming?: boolean, cancelled = false): readonly DsSegment[] {
  const committedRef = useRef<Projection | null>(null);
  const committed = committedRef.current;
  const appending = committed && committed.authority.id === authority.id && committed.authority.trusted === authority.trusted && text.startsWith(committed.scanner.text);
  const generation = committed ? appending ? committed.generation : committed.generation + 1 : 0;
  const scanner = appending && committed ? committed.scanner.fork() : new DsBlockScanner({ ...authority, id: `${authority.id}:${generation}` });
  if (text.length > scanner.text.length && scanner.state !== "cancelled") scanner.append(text.slice(scanner.text.length));
  if (scanner.state === "streaming" && (cancelled || streaming !== true)) scanner.finish(cancelled ? "cancelled" : "ended");
  const projection: Projection = { scanner, authority: { ...authority }, generation };
  useLayoutEffect(() => { committedRef.current = projection; });
  return scanner.segments;
}
