import { afterEach, describe, expect, it } from "vitest";
import { __setInvokeForTest } from "./bridge";
import type { Notification } from "./notificationTypes";
import {
  answerRemoteNotification,
  deleteRemoteNotification,
  readRemoteNotification,
  resolveRemoteNotification,
} from "./remoteNotifications";

const remote: Notification = {
  id: "remote-notification/install-a/source%2Fid",
  title: "Waiting",
  body: null,
  severity: "Warning",
  tab_id: "mesh:install-a/main",
  workspace_id: null,
  timestamp: 1,
  read: false,
  peer_name: "Same name",
  remote_origin: { peerId: "install-a", sourceId: "source/id", peerName: "Same name" },
};

afterEach(() => __setInvokeForTest(null));

describe("remote notification routes", () => {
  it("reads, deletes, resolves and answers only the row's exact source route", async () => {
    const calls: Array<{ command: string; args: Record<string, unknown> | undefined }> = [];
    __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      return { answered: "Approve", delivered: true } as T;
    });

    await readRemoteNotification(remote);
    await deleteRemoteNotification(remote);
    await resolveRemoteNotification(remote);
    await answerRemoteNotification(remote, "Approve");

    expect(calls).toEqual(["read", "delete", "resolve", "answer"].map(notificationAction => ({
      command: "mesh_peer_notification_action",
      args: { peerId: "install-a", sourceId: "source/id", notificationAction, ...(notificationAction === "answer" ? { label: "Approve" } : {}) },
    })));
  });

  it("refuses a tampered projection before routing any action", async () => {
    const calls: unknown[] = [];
    __setInvokeForTest(async <T>(command: string) => {
      calls.push(command);
      return {} as T;
    });
    await expect(readRemoteNotification({ ...remote, id: "same-source-id" })).rejects.toThrow("origin is missing or invalid");
    expect(calls).toEqual([]);
  });
});
