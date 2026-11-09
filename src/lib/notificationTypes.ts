/** What one notification button does, as the daemon stores and emits it. */
export type NotificationAction = import("../../gen/NotificationAction").NotificationAction;

export type CallerNotificationAction = import("../../gen/CallerNotificationAction").CallerNotificationAction;

export type NotificationPromptIdentity = import("../../gen/PromptIdentity").PromptIdentity;

/** The answer slot of one notification: the tab captured when the question was
 * sent, and the single label it may ever hold. */
export type NotificationAnswer = Omit<import("../../gen/AnswerSlot").AnswerSlot, "answered_via"> & Partial<Pick<import("../../gen/AnswerSlot").AnswerSlot, "answered_via">>;

export type Notification = Omit<import("../../gen/Notification").Notification, "answer" | "target_tab_id" | "chat_message_id"> & {
target_tab_id?: string | null;
chat_message_id?: string | null;
answer?: NotificationAnswer;
lifetime_ms?: number;
ephemeral?: boolean;
update_only?: boolean;
local_actions?: { label: string; run: () => void | Promise<void> }[];
key?: string;
icon_svg?: string | null;
icon_color?: string | null;
icon_hint?: string | null;
answer_pending?: boolean;
answer_error?: string;
};

