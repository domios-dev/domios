import { useEffect, useRef } from "react";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { loadRemoteNotifications } from "../../lib/remoteNotifications";
import { publishRemoteNotificationArrival } from "../../lib/notificationSound";
import type { Notification } from "../../lib/types";
import type { StreamEnd } from "../../lib/transportClient";
import type { PairedRoster } from "./useMeshPeerProjection";
import { createRemoteNotificationSource, type RemoteNotificationCursor, type RemoteNotificationEventHandlers, type RemoteNotificationSourceAction } from "./remoteNotificationSource";

const ports = {
  load: loadRemoteNotifications,
  arrival: (notification: Notification) => publishRemoteNotificationArrival(notification.level),
  subscribe(peerId: string, cursor: RemoteNotificationCursor | null, handlers: RemoteNotificationEventHandlers) {
    const controller = new AbortController();
    const since = cursor ? "?since=" + encodeURIComponent(cursor.boot + ":" + cursor.seq) : "";
    const stop = daemonTransportForPeer(peerId).subscribe(
      "/api/events/stream" + since,
      (_event, value) => handlers.frame(value),
      {
        signal: controller.signal,
        raw: true,
        onReady: handlers.ready,
        onEnd: (end: StreamEnd) => handlers.end(end),
      },
    );
    return () => {
      controller.abort();
      stop();
    };
  },
};

export function useRemoteNotificationSource({
  roster,
  dispatch,
}: {
  roster: PairedRoster | null;
  dispatch: (action: RemoteNotificationSourceAction) => void;
}): void {
  const rosterRef = useRef(roster);
  const dispatchRef = useRef(dispatch);
  const sourceRef = useRef<ReturnType<typeof createRemoteNotificationSource> | null>(null);
  rosterRef.current = roster;
  dispatchRef.current = dispatch;

  useEffect(() => {
    const source = createRemoteNotificationSource(action => dispatchRef.current(action), ports);
    sourceRef.current = source;
    source.sync(rosterRef.current);
    return () => {
      source.dispose();
      if (sourceRef.current === source) sourceRef.current = null;
    };
  }, []);

  useEffect(() => {
    sourceRef.current?.sync(roster);
  }, [roster]);
}
