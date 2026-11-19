import { openNeighbor } from "./neighborFocus";
import { useNeighborPeer, useNeighborProjection } from "./neighborProjection";

export function ConciergeChatHint({ friendship }: { friendship: string }) {
  const peer = useNeighborPeer();
  const projection = useNeighborProjection(peer ?? "local", friendship);
  const friend = projection[friendship]?.friend_display_name ?? "your friend";
  return <div className="ct-composer-lane mx-auto w-full px-3.5 py-2 text-sm text-[var(--ct-muted-fg)]">
    Concierge for {friend} — <a className="underline" href={`domios://neighbor/${friendship}`} onClick={event => { event.preventDefault(); openNeighbor(friendship); }}>message {friend} in Neighbors</a>
  </div>;
}
