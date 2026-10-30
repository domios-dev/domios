import { DsBlockScanner, type DsAuthority, type DsLifecycle, type DsSegment } from "@codeterm/chat-core";
import { useLayoutEffect, useRef } from "react";

interface Projection { scanner: DsBlockScanner; authority: DsAuthority; generation: number }
interface CommittedSource { text: string; authority: DsAuthority; generation: number; state: DsLifecycle }
export function useDsSegments(text: string, authority: DsAuthority, streaming?: boolean, cancelled = false): readonly DsSegment[] {
  const ref = useRef<Projection | null>(null);
  const committedRef = useRef<CommittedSource | null>(null);
  let value = ref.current;
  let restoredState: DsLifecycle | undefined;
  const replaced = value && (value.authority.id !== authority.id || value.authority.trusted !== authority.trusted || !text.startsWith(value.scanner.text));
  if (!value || replaced) {
    const committed = committedRef.current;
    const returningToCommitted = committed && committed.authority.id === authority.id && committed.authority.trusted === authority.trusted && committed.text === text;
    if (returningToCommitted && committed) restoredState = committed.state;
    const generation = returningToCommitted && committed ? committed.generation : value ? value.generation + 1 : 0;
    value = { scanner: new DsBlockScanner({ ...authority, id: `${authority.id}:${generation}` }), authority: { ...authority }, generation };
    ref.current = value;
  }
  const scanner = value.scanner;
  if (text.length > scanner.text.length && scanner.state !== "cancelled") scanner.append(text.slice(scanner.text.length));
  if (restoredState === "ended" || restoredState === "cancelled") scanner.finish(restoredState);
  if (scanner.state === "streaming" && (cancelled || streaming !== true)) scanner.finish(cancelled ? "cancelled" : "ended");
  const committedText = scanner.text;
  const generation = value.generation;
  const state = scanner.state;
  useLayoutEffect(() => {
    committedRef.current = { text: committedText, authority: { id: authority.id, trusted: authority.trusted }, generation, state };
  }, [committedText, authority.id, authority.trusted, generation, state]);
  return scanner.segments;
}
