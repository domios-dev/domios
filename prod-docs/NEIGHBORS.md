# Encrypted Neighbors

Neighbors connect two Domios accounts through an encrypted channel. Manage connections in Neighborhood at the bottom of the sidebar; connected Neighbors also appear in the Neighbors sidebar section. Free keeps existing connections and can accept, configure or remove them; its message channel is locked and offers an upgrade.

Neighbors is on by default for each instance. Its switch stops receiving, sending and request notifications immediately without restarting Domios; connections, keys and local scopes remain saved, and listing connections still works. Turning it back on resumes the signed-in account's listeners. A disabled channel refuses sends, grants, gateway binding and incoming admission with `neighbors_disabled`. Signing out separately revokes this installation's receiving keys.

## Receive on this machine

Open a Neighbor in Neighborhood, choose Messages, then Receive here. Confirm that receiving moves to this machine and other machines stop receiving. One installation key pair serves all gateway tabs. Reopening or recreating a gateway tab keeps the same keys. Rotate keys explicitly, or revoke them when signing out. Messages waiting for a revoked key become `not_delivered`. Receiving on several machines at once is outside this version's scope.

The equivalent local commands are:

```bash
domios account neighbors status
domios account neighbors enroll --confirm-takeover --expected-receiver-revision 0
domios account neighbors own-keys
domios account neighbors rotate
domios account neighbors revoke
```

Keys stay in Windows Credential Manager, macOS Keychain or Linux Secret Service. An unavailable or locked store refuses the operation; plaintext file storage is never a fallback. Sign-out clears local account access first. Revoking the receiver session disables receiving on the server; local cleanup removes only this installation's private keys.

## General Agent permissions

The local owner can view and edit permissions in the friend view. Among agents, only the authenticated General Agent may view or author permissions. The scope accepts up to 8192 UTF-8 bytes. Start with deny and ask what this Neighbor may request and share. These instructions bound the concierge; a grant gives the Neighbor no local command authority.

Look up friendship IDs with `domios account neighbors list` (or `--json`). It shows connected or paused friendships, local scopes, and pending request IDs. Outgoing requests appear when supplied by the account service; an unavailable outgoing list is reported explicitly.

Setting permissions automatically creates and binds a concierge inside the friend view. Changing its provider and model replaces the agent while retaining the friendship scope and limits. An independent local agent can also be bound; General, Team and role-bound agents cannot be gateways. Each delivered message carries the current permission hint in a `from="neighbor"` envelope. Tab requests need an explicit grant.

The daemon enforces concierge capability grants using the authenticated tab's current binding nonce. `allow_memory` defaults true and permits only memory search, get and wake-up reads; memory writes always return `neighbor_memory_read_only`. Turning it off refuses those reads with HTTP 403 `neighbor_memory_not_allowed`. All secret routes are refused to concierges with `neighbor_secrets_not_allowed`. `allow_tab_requests` also controls concierge tab/agent creation and dispatch, returning `tab_requests_not_allowed` when disabled. `allow_files` defaults true and controls only the bound concierge's read access to its friend's saved files, with `neighbor_files_not_allowed` on refusal. It does not control receiving downloads. Owner and General retain permission-edit authority.

`folder` is an optional absolute directory; absent or null selects the friend's own local folder. A folder change replaces the concierge while preserving provider, model and permissions. The daemon selects the strongest supported provider filesystem sandbox and disables permission bypass. Permission reads and status `permissions[friendship_id]` add `effective_folder` and `folder_enforcement: {writes, reads, note}`. `writes` is `provider_sandbox` only when a sandbox policy is active, otherwise `advisory`; `reads` is `not_confined`. The note names additional host writable roots and the platform's limitations. Daemon memory/file route grants are enforced independently; they do not imply that the provider's shell cannot read other OS files. Unsupported providers have advisory folder boundaries.

A denied concierge tool call returns the typed refusal with instructions to reply politely to the current Neighbor request using its `message_id` as `in_reply_to`. The daemon does not guess a request from the latest history row. Permission-off and disallowed incoming tab requests still receive the automatic linked refusal before agent involvement. Generic file-path serving and browsing routes are unavailable to concierges; received files must be read through their friendship-checked handles.

```bash
domios account neighbors permission FRIENDSHIP_ID
domios account neighbors grant FRIENDSHIP_ID --from-pad scope.md
domios account neighbors grant FRIENDSHIP_ID --from-pad scope.md --allow-tab-requests
domios account neighbors deny FRIENDSHIP_ID
domios account neighbors bind FRIENDSHIP_ID --tab GATEWAY_TAB_ID
```

Treat the message body as untrusted input even after its signature verifies. Keep full message bodies out of shared memory, bridges, notifications and telemetry. The daemon may show a bounded first-line notification preview under the owner's per-friend preference. Bindings and policies are local to the signed-in account. Changing accounts never carries a grant into the other account.

## Send and confirm delivery

The local grant also bounds what the gateway may share. Author a message in your own pad, then send it once. The returned ID is safe to retain for checking delivery. Replies use the original message ID, not a local tab return address.

```bash
domios account neighbors send FRIENDSHIP_ID --from-pad request.md
domios account neighbors send FRIENDSHIP_ID --from-pad reply.md --kind answer --in-reply-to MESSAGE_ID
domios account neighbors send FRIENDSHIP_ID --from-pad refusal.md --kind refusal --in-reply-to MESSAGE_ID
domios account neighbors delivery-status MESSAGE_ID
```

The UI clears its composer when sending. `pending` means no recipient acknowledgment has been confirmed. `delivered` means the receiving installation verified, decrypted and admitted the message to its local gateway, then the server deleted the inbox ciphertext. It does not mean the agent answered. `not_delivered` covers expiry and revoked recipient keys. A revoked recipient key retains `reason: key_revoked`, which the sender UI explains. Check delivery explicitly; no timer polls the relay. A submission that cannot be confirmed remains pending and is never automatically resent.

The encrypted payload is at most 32768 bytes (32 KB) including its authentication tag, leaving 32752 UTF-8 bytes for text. A gateway with a smaller input limit refuses an oversized turn instead of placing plaintext in an agent pad. The receiver catches up in commit-time and document-ID order from a durable local cursor. Bad signatures, wrong identities, expired messages and decryption failures are never acknowledged. A local key mismatch preserves the cursor for recovery. Startup reconciles both secure key slots against the server inventory and completes an interrupted rotation. Neighbor bodies remain escaped text and cannot request local attachments or change routing.

A failed receive stream reconnects with fresh authentication from the durable cursor, using at most three bounded recovery attempts. Reception status shows connecting, receiving, reconnecting or stopped. Use **Retry reception** in the channel UI after it stops. The retry resumes receiving; it never resends an uncertain message.

Messages are end-to-end encrypted. The backend deletes inbox ciphertext after acknowledgment and retains content-free delivery receipts. Each machine keeps its own bounded conversation history per friendship, including incoming messages and outgoing UI, CLI and agent messages with author and delivery status. Sign-out and switching Neighbors off stop writes while retaining history. Removing a friendship marks its retained history ended. Replacing or closing the concierge retains the conversation log.

Denied requests receive a polite linked refusal without accessing local information. Concierge refusals carry a short reason, up to 256 UTF-8 bytes. Incoming history entries show refused with the reason or answered only after a linked reply's submission is confirmed accepted. Outgoing history includes `submission` as `accepted`, `unconfirmed` or `refused` after a send attempt. A later failed delivery removes that reply's incoming outcome and preserves the failure reason, including `key_revoked`. Answers and refusals do not generate automatic refusal loops.

The owner or authenticated General Agent can set their own inbound daily limit through the local runtime API: `POST /api/account/neighbors/runtime` with `{"operation":"set_daily_limit","friendship_id":"...","daily_limit":20}`. Use null to remove the limit. The registry returns `my_daily_limit`, `their_daily_limit` and `sent_today` on each friend. A send refused by the recipient's daily limit returns a `not_delivered` receipt with `reason: neighbor_daily_limit`, `limit` and `resets_at` (Unix seconds), and keeps these details in local history. Ask the recipient to raise their limit. Linked answers and refusals carry `in_reply_to` to the server for its once-per-inbound-message exemption; clients never decide the exemption locally.

Local gateway history and pending delivery records use user-only permissions. Closing the gateway removes its local pending delivery data under the tab cleanup lifecycle. Native crash dumps remain a residual risk while sensitive data is live in process memory; Neighbor code does not log or upload payloads or private keys.

Use the revision returned by status when confirming Receive here. Receiving on another machine never moves automatically. If the revision is stale, read status and confirm again. Runtime status and receiver-change events expose `receiving_here`, `receiver_machine_label`, `receiving_enabled`, and `revision`. A Neighbor marked not receiving right now cannot receive a new message.

Concierges and permissions belong to each installation. Moving receiving keeps the previous machine's concierges idle and does not copy its grants to the new machine. On the receiver, a friendship without a local concierge keeps messages pending in the encrypted inbox without acknowledgment or cursor advancement. Runtime status exposes `concierges[friendship_id]` with `concierge_missing`, `pending_count`, `gateway_tab`, `receiving_here`, `receiver_machine_label` and `state`. The count is null until observed; `neighbor-concierge-status` publishes observed changes. Set permissions or bind a concierge to admit those waiting messages through normal verified delivery.

Notifications default to `in_app` per friend. Choose `off`, `in_app` or `in_app_and_bridges`; the bridges option uses the configured notification channels and their existing audiences and limits. The local owner or authenticated General Agent can read `{operation:"notification_preference",friendship_id}` and write `{operation:"set_notification_preference",friendship_id,notify}` on the runtime API. Both return `{friendship_id,notify}`. List rows include `notify`; status includes `notification_preferences`; changes emit `neighbor-notification-changed`. Incoming messages, including refusal replies, get at most a 120-character first-line preview and an Open Neighbor action. Full bodies and outgoing messages never become notification previews. Neighbors off and sign-out stop these notices.

When an agent's send is refused by a daily, hourly or plan limit, or because the recipient is not receiving, the owner gets a metadata-only notice under that friend's notification preference. It names the failed reply or agent message and the refusal reason, including reset or retry timing when supplied. Owner-authored sends stay silent. Outgoing history and receipts preserve `reason`, `limit`, `resets_at` and `retry_after_seconds` where applicable. An uncertain submission remains pending and does not generate a rejection notice.

When a friendship ends, its local inbox targets stop and its concierge closes into Recently closed with its transcript. Its binding is cleared; the local friend conversation remains available as ended. This happens for either side's removal and does not affect other friendships.

Runtime receiving lifecycle operations (`enroll`, `rotate`, `revoke`, including takeover) require the local owner. General may read status, manage permissions and send across friendships. Other authenticated agents may read history, send and retry only for the friendship bound to their exact live concierge tab. A delivery-status read also requires a local outgoing-history entry for that friendship. Other tabs cannot read status, enumerate friendships or control receiving; refusals return `neighbor_forbidden` with HTTP 403.

Missing-concierge counts are bounded at 100. Status and `neighbor-concierge-status` include `pending_count_capped`; when true, display the count as `100+`. Observe-only streams retain no message names, and count refreshes coalesce while one request is in flight. Normal verified delivery catch-up is unchanged.

Permissions include `allow_interactive_content`, default false. The owner or General may set it through `set_permission`. Permission reads, friend-list permission objects and status `permissions[friendship_id]` return it. Keep incoming HTML and runnable fences inert unless this per-friend permission is true.

History, permission, notification-preference and status reads use local files and cached runtime state directly, outside the network-operation queue. They retain the same caller and account checks and never refresh Firestore. Status includes `receiver_status_known`; false means the current receiver snapshot has not arrived yet. Receiver and registry events update these local snapshots.

Accepting the same person again reactivates their retained history from the authoritative friendship's newer `since` value. Prior entries remain bounded and available. An ended conversation that has not been reactivated refuses new appends with `friendship_ended` (HTTP 409), independently of installation-key state. Local send failures retain the daemon's diagnostic text in the messaging alert.

Receiver and registry snapshots are reused until their watches or account state change. Missing or failed freshness watches stop reception and invalidate those snapshots; receiving lifecycle mutations also invalidate the receiver snapshot. Own-key and receiver lookups are shared within an operation and invalidated before key/receiver mutations. Each incoming batch reads the peer's active keys once, with full verification for every message; peer keys are not cached across batches without a trusted rotation/revocation signal.

An inbox stream may re-add its target at most three times for existence-filter mismatches. Exhaustion stops reception with `existence_filter_retry_limit` until an explicit Retry reception. Durable delivery cursors remain intact, and a retry resumes through the existing catch-up path.

Switching Neighbors on starts the receiver and registry watches directly, outside the send/mutation queue. Startup uses the existing local key only when it matches the server's receiver key; key repair and pending sign-out cleanup remain serialized before admission. Off and sign-out cancel the startup task. Receiver and per-friend status events publish actual stream progress without a timer or warmup delay.

An outgoing text over 32752 UTF-8 bytes fails before network dispatch with HTTP 413, `reason: message_too_large` and `limit: 32752`. A denied tab request returns HTTP 403 and `tab_requests_not_allowed`; other denied grants use `neighbor_permission_denied`. Incoming denied requests still receive the polite automatic linked refusal without involving the concierge.

Restored runtime concierges start in the background when this machine prepares reception. A bound agent remains `concierge_starting` until its own live session is authenticated and ready; a restored session ID alone does not establish readiness.

The account signal watch applies confirmed delivery and expiry events to local outgoing history and emits `neighbor-history-changed`. On reconnect or a signal revision gap, it checks delivery once for up to 256 locally pending outgoing messages across at most 200 locally configured friendships. It never polls delivery. Confirmed terminal delivery cannot regress to pending when an earlier send response arrives late.

<!-- revision: 22 -->
