# Chat UI delayed send

Status: IMPLEMENTED

## Goal

Allow a user to schedule a Chat UI message for a future time without changing the
default send path. Pressing Enter or the normal Send button must write to the pane PTY
immediately; delayed send is explicit only.

## UX

- Normal send: immediate, no local batching, no hidden queue.
- Delayed send: hold or open the Send button menu, choose a relative or absolute time,
  then confirm.
- Composer chrome shows a small `N queued` chip next to the `terminal` chip when the
  pane has pending scheduled messages.
- Clicking the chip opens a popover with queued messages, scheduled time, target pane,
  and actions: edit text, change time, send now, delete.

## Backend

- Persist queued messages in the daemon store so restart does not lose them.
- Store target pane id, workspace id, text, scheduled fire time, created time, and
  state.
- Delivery uses the same PTY send path as normal Chat UI send at fire time.
- If the target pane is gone, mark failed and surface it in the popover instead of
  silently dropping the message.

## Edge Cases

- `/new` or `/clear` before fire: keep the queued message bound to the pane, but show a
  warning in the popover if the provider session id changed since scheduling.
- Multiple queued messages for the same time: fire in creation order, as separate PTY
  writes.
- App restart while overdue: deliver after boot once the pane is restored, or mark
  failed if it cannot be restored.

## Tests

- Unit: scheduler ordering, edit/delete/send-now transitions, restart rehydration.
- API: create/list/update/delete/fire due messages with a fake clock.
- Playwright smoke: schedule a message, see `N queued`, edit it, send now, verify it
  appears as a normal user bubble and does not affect immediate sends.

Implemented surface:

- Tauri commands: `delayed_send_list`, `delayed_send_create`,
  `delayed_send_update`, `delayed_send_delete`, `delayed_send_send_now`.
- REST: `GET/POST /api/panes/{id}/delayed-messages`,
  `PATCH/DELETE /api/delayed-messages/{id}`,
  `POST /api/delayed-messages/{id}/send-now`.
- UI: clock button next to Send, `N queued` chip next to `terminal`, queue popover
  with edit, send-now and delete.

<!-- revision: 2 -->
