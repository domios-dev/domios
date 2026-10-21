import { DEV_BUILD } from "./devBuild";
import type { Notification } from "./notificationTypes";
import { uid } from "./uid";

export interface Notice {
  kind: "toast" | "debug";
  severity: Notification["severity"];
  title: string;
  detail?: string;
  actions?: Notification["local_actions"];
  id?: string;
}

export type ToastInput = Omit<Notification, "timestamp" | "read">;

export interface NoticePorts {
  upsertToast: (toast: ToastInput) => void;
  debug?: (notice: Notice) => void;
}

let toastPort: NoticePorts["upsertToast"] | null = null;
const pending = new Map<string, ToastInput>();

export function connectNoticeToasts(upsertToast: NoticePorts["upsertToast"]) {
  toastPort = upsertToast;
  for (const toast of pending.values()) upsertToast(toast);
  pending.clear();
  return () => { if (toastPort === upsertToast) toastPort = null; };
}

function debugNotice(notice: Notice) {
  if (DEV_BUILD) console.debug("[notice]", notice.severity, notice.title, notice.detail ?? "");
}

export function createNoticePublisher({ upsertToast, debug = debugNotice }: NoticePorts) {
  return (notice: Notice): void => {
    if (notice.kind === "debug") {
      if (DEV_BUILD) debug(notice);
      return;
    }
    upsertToast({
      id: notice.id ?? uid(),
      severity: notice.severity,
      level: notice.severity === "Error" ? "failure" : notice.severity === "Warning" ? "action_needed" : "verbose",
      title: notice.title,
      body: notice.detail ?? null,
      local_actions: notice.actions,
      tab_id: null,
      workspace_id: null,
    });
  };
}

export const publishNotice = createNoticePublisher({
  upsertToast: (toast) => {
    if (toastPort) toastPort(toast);
    else {
      pending.set(toast.id, toast);
      while (pending.size > 3) pending.delete(pending.keys().next().value!);
    }
  },
});
