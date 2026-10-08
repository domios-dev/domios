/** What one notification button does, as the daemon stores and emits it. */
export type NotificationAction =
  | { type: "url"; label: string; value: string }
  | { type: "dismiss"; label: string }
  | { type: "answer"; label: string }
  | {
      type: "respond_prompt_option";
      label: string;
      value: string;
      identity: NotificationPromptIdentity;
    };

export type CallerNotificationAction =
  | Extract<NotificationAction, { type: "url" }>
  | Extract<NotificationAction, { type: "dismiss" }>
  | Extract<NotificationAction, { type: "answer" }>;

export interface NotificationPromptIdentity {
  kind: "provider" | "directory_trust" | "structured" | "manual_update";
  options: Array<{ label: string; value: string }>;
}

/** The answer slot of one notification: the tab captured when the question was
 * sent, and the single label it may ever hold. */
export interface NotificationAnswer {
  asked_from_tab: string | null;
  answered: string | null;
  /** Whether the standing answer reached the asking tab. Recording an answer and
   * carrying it back are separate facts, so a refused send never reads as done. */
  delivered: boolean;
}

export interface Notification {
  blueprint_run?: import("./blueprintRun").BlueprintRunPopup;
  lifetime_ms?: number;
  ephemeral?: boolean;
  update_only?: boolean;
  local_actions?: { label: string; run: () => void | Promise<void> }[];
  id: string;
  key?: string;
  tab_id: string | null;
  workspace_id: string | null;
  /** Navigation hint: the tab whose chat holds the message this is about. */
  target_tab_id?: string | null;
  /** The exact `data-message-id` inside `target_tab_id`; never matched by text. */
  chat_message_id?: string | null;
  title: string;
  body: string | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
  /** Wire and dedupe axis only: it joins provenance and title in the record's
   *  identity. No surface tones by it — `level` is the single tone axis. */
  severity: "Info" | "Warning" | "Error";
  /** The tone axis, named by the sender and routed on by the destination
   *  matrix. Absent means the caller named no level — a distinct, legal state,
   *  not a lost one — and every surface paints it the neutral tone. */
  level?: "verbose" | "action_needed" | "failure";
  timestamp: number;
  read: boolean;
  /** Actions remain available in the bell after toast expiry. */
  actions?: NotificationAction[];
  /** Present when the notification offers a choice */
  answer?: NotificationAnswer;
  resolved?: boolean;
  /** Sender hint; the host still caps every toast lifetime. */
  sticky?: boolean;
}

