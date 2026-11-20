import { useMemo, useRef, type Dispatch, type RefObject } from "react";
import { invoke } from "../../lib/bridge";
import { sendNotificationAnswer, type AnswerResult } from "../../lib/notificationAnswerSend";
import { isRemoteNotification } from "../../lib/notificationAdmission";
import { answerRemoteNotification, deleteRemoteNotification, readRemoteNotification, remoteNotificationRoute } from "../../lib/remoteNotifications";
import type { Notification } from "../../lib/types";
import type { Action } from "./reducer";
import type { ConfirmedRemoteNotificationAction, RemoteNotificationRouteLease } from "./remoteNotificationSource";
import type { ExtendedState } from "./tree";
import type { PairedRoster } from "./useMeshPeerProjection";
import { useRemoteNotificationSource } from "./useRemoteNotificationSource";

interface NotificationPorts {
  notifications: () => Notification[];
  roster: () => PairedRoster | null;
  dispatch: Dispatch<Action>;
  captureRoute: (peerId: string, publicKey: string) => RemoteNotificationRouteLease | undefined;
}

export function createMuxNotificationActions({ notifications, roster, dispatch, captureRoute }: NotificationPorts) {
  const pendingAnswers = new Set<string>();
  const pendingChanges = new Set<string>();
  const find = (id: string) => notifications().find(row => row.id === id);
  const routeFor = (peerId: string) => {
    const publicKey = roster()?.peers.paired.find(peer => peer.instance_id === peerId)?.public_key;
    return publicKey ? captureRoute(peerId, publicKey) : undefined;
  };

  const changeRemote = (row: Notification | undefined, action: Exclude<ConfirmedRemoteNotificationAction, { type: "UPDATE_NOTIFICATION_ANSWER" }>) => {
    const origin = row && remoteNotificationRoute(row);
    if (!row || !origin) return;
    const route = routeFor(origin.peerId);
    const pendingKey = `${action.type}:${action.id}`;
    if (!route || pendingChanges.has(pendingKey)) return;
    const send = action.type === "MARK_READ" ? readRemoteNotification : deleteRemoteNotification;
    pendingChanges.add(pendingKey);
    return send(row).then(() => {
      route.confirm(action);
    }).catch(error => console.warn("Remote notification action failed:", error))
      .finally(() => pendingChanges.delete(pendingKey));
  };

  const persistLocalRead = (id: string) => invoke<void>("mark_notification_read", { id });
  const markRead = (id: string) => {
    const row = find(id);
    const action = { type: "MARK_READ", id } as const;
    if (isRemoteNotification(row ?? { id })) return changeRemote(row, action);
    dispatch(action);
    return persistLocalRead(id).catch(error => console.warn("Failed to mark notification read:", error));
  };

  const markAllRead = () => {
    const unreadLocals: string[] = [];
    const pendingRemote: Array<Promise<void> | undefined> = [];
    for (const row of notifications()) {
      if (row.read) continue;
      if (isRemoteNotification(row)) pendingRemote.push(changeRemote(row, { type: "MARK_READ", id: row.id }));
      else unreadLocals.push(row.id);
    }
    dispatch({ type: "MARK_ALL_READ" });
    return Promise.all([...pendingRemote, ...unreadLocals.map(persistLocalRead)])
      .catch(error => console.warn("Failed to mark all notifications read:", error));
  };

  const deleteNotification = (id: string) => {
    const row = find(id);
    const action = { type: "DELETE_NOTIFICATION", id } as const;
    if (isRemoteNotification(row ?? { id })) return changeRemote(row, action);
    dispatch(action);
    return invoke<void>("clear_notifications", { id }).catch(error => console.warn("Failed to delete notification:", error));
  };

  const clearNotifications = () => {
    const rows = notifications();
    const pending = rows.filter(isRemoteNotification).map(row => changeRemote(row, { type: "DELETE_NOTIFICATION", id: row.id }));
    if (rows.some(row => !isRemoteNotification(row))) {
      dispatch({ type: "CLEAR_NOTIFICATIONS" });
      pending.push(invoke<void>("clear_notifications").catch(error => console.warn("Failed to clear notifications:", error)));
    }
    return Promise.all(pending);
  };

  const answerNotification = (id: string, label: string) => {
    const row = find(id);
    const origin = row && remoteNotificationRoute(row);
    const route = origin ? routeFor(origin.peerId) : undefined;
    if (isRemoteNotification(row ?? { id }) && (!row || !route)) return;
    return sendNotificationAnswer(
      id, label, pendingAnswers, row,
      (answerId, answerLabel) => row && origin
        ? answerRemoteNotification(row, answerLabel)
        : invoke<AnswerResult>("answer_notification", { id: answerId, label: answerLabel }),
      (answerId, update) => {
        if (!route || "pending" in update) {
          if (!route || route.isCurrent()) dispatch({ type: "UPDATE_NOTIFICATION_ANSWER", id: answerId, update });
        } else {
          route.confirm({ type: "UPDATE_NOTIFICATION_ANSWER", id: answerId, update });
        }
      },
    );
  };

  return { markRead, markAllRead, deleteNotification, clearNotifications, answerNotification };
}

export function useMuxNotificationActions(state: RefObject<ExtendedState>, roster: PairedRoster | null, dispatch: Dispatch<Action>) {
  const rosterRef = useRef(roster);
  rosterRef.current = roster;
  const captureRoute = useRemoteNotificationSource({ roster, dispatch });
  return useMemo(() => createMuxNotificationActions({
    notifications: () => state.current.notifications,
    roster: () => rosterRef.current,
    dispatch,
    captureRoute,
  }), [state, dispatch, captureRoute]);
}
