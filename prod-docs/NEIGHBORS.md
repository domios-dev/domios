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

Setting permissions automatically creates and binds a concierge inside the friend view. Changing its provider and model replaces the agent while retaining the friendship scope and limits. An independent local agent can be bound only when its launch already contains the current concierge initial instructions; General, Team and role-bound agents cannot be gateways. Permissions live in the initial prompt; each delivered message carries a compact `from="neighbor"` envelope. Tab requests need an explicit grant.

The daemon applies a default-deny command allowlist to the authenticated concierge credential at socket, HTTP RPC and direct HTTP entry points. A caller field or header cannot widen that authority. Allowed operations are its own friendship's history/send/retry/delivery status, reporting and answering its own request, granted memory reads, and granted reads of its own friend's files. Nonce-bound lifecycle hooks (session start, stop, context and permission) may report only on their own tab, including during launch. They add no command authority, and concierges cannot answer their own permission prompts to approve tools. A `workspaces` grant permits listing and sending messages to agent tabs in those workspace IDs only. Shell tabs and other friends' concierges stay excluded. Null or an empty list grants no workspace access. Listings omit session credentials and unrelated workspace counts; remote tabs are not included. Raw terminal actions and batch sends remain refused. Every delegated message carries `from="neighbor"`, the friend name and `via="concierge"`, plus a host note identifying the friend request. Other routes, including secrets, memory writes, receiver keys, configuration, cron, mesh, plugins, bridges and notifications, return HTTP 403 `neighbor_capability_denied`.

`allow_memory` defaults true and permits memory search, get and wake-up. Turning it off returns `neighbor_memory_not_allowed`. `memory_areas` is null for all non-secret memory or a list of current taxonomy area names. Restricted searches return only those areas; a caller's area filter can narrow but cannot widen the grant. A foreign get and global wake-up are refused with `neighbor_memory_area_denied`; use scoped search instead. Unknown area names are rejected when saving permissions with `neighbor_memory_area_unknown`.

`allow_tab_requests` controls incoming tab requests and concierge creation requests, returning `tab_requests_not_allowed` when disabled. Ordinary agent creation also requires an explicit granted workspace and inherits friendship authority and the configured machine access before launch. Paths that cannot preserve both are refused with `neighbor_capability_denied` even when enabled. No unrestricted descendant is created. `allow_files` defaults true and permits only the bound concierge's list/open reads through friendship-checked attachment handles, returning `neighbor_files_not_allowed` on refusal. Downloads are independent of this grant. Owner and General retain permission-edit authority.

`folder` is an optional absolute directory; absent or null selects the friend's own local folder. `machine_access` accepts `none`, `read_only`, `folder` and `full`. The current provider contract cannot disable all shell/tool execution, so `none` is refused with `neighbor_machine_access_unsupported`, and the default is `read_only`. That level uses the provider's read-only sandbox when supported; it still permits command execution and reads of this OS user's files. `folder` requires an explicit folder and prefers the provider's workspace-write sandbox, falling back to read-only confinement when available. When the provider or platform supports neither sandbox, the concierge starts in advisory mode: the working folder does not confine reads or writes. The returned `folder_enforcement` describes the actual enforcement. `full` explicitly removes the sandbox. A folder or machine-access change replaces the concierge while retaining provider, model and permissions. Startup also replaces older concierges whose working folder or confinement does not match the current permission.

Permission reads and status `permissions[friendship_id]` add `effective_folder` and `folder_enforcement: {writes, reads, network, note}`. Writes are `pad_only` when the read-only concierge has a provider-supported exception for its own AgentPad, `provider_sandbox` for a folder sandbox, `unrestricted` for explicit full access, otherwise `advisory`. A supported read-only runtime keeps the friend folder read-only and permits reply files only in its own AgentPad; folder mode permits the folder and that pad. Other pads, build caches and Git metadata are not added. Reads are `not_confined`; network is `disabled` only when the runtime explicitly confirms it, otherwise `not_confined`. The note includes writable roots and platform limitations. These daemon grants do not prevent an agent shell from reading plaintext OS files. Stdin replies with `domios account neighbors send FRIENDSHIP_ID --from-pad -` also work when the provider has no writable-pad exception.

Concierge authority is host-owned, attached before launch and persisted with the tab. Restart, sign-out, role changes and a missing/ended binding cannot turn it into an unrestricted agent. An existing independent agent may be bound only when its current folder and sandbox already match the permission; use permission editing or agent replacement to start the configured launch. Binding never silently reuses a broader launch.

A denied concierge tool call returns the typed refusal with instructions to reply politely to the current Neighbor request using its `message_id` as `in_reply_to`. The daemon does not guess a request from the latest history row. Permission-off and disallowed incoming tab requests still receive the automatic linked refusal before agent involvement. Generic file-path serving and browsing routes are unavailable to concierges; received files must be read through their friendship-checked handles.

CLI capability denials preserve the `neighbor_*` reason and the daemon's bounded explanation. JSON output retains the general `error.code: refused` category and adds the specific `error.reason`; permission refusal exits with 1. Secret backend outages retain exit 6 and withhold structured secret payloads.

After editing a friend, General may include a link in its reply to open that friend's window: `domios://neighbor/<friendship_id>/permissions` or `domios://neighbor/<friendship_id>/concierge`. Use the actual 64-character lowercase hexadecimal friendship ID; the only section names are `permissions` and `concierge`. Markdown links with these URLs open the local friend window. The plain `domios://neighbor/<friendship_id>` URL opens the conversation. Settings-related notifications use the permissions link; arrival notifications open the conversation. These links only navigate; they do not change a grant.

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

## Incoming message handling

Each incoming history row may include `handling_state`, optional `handling_reason`, and `handling_updated_at` (Unix milliseconds). States are `waiting`, `delivered_to_concierge`, `working`, `answered`, `refused`, `no_reply`, `failed`, and `stopped`. Waiting rows identify `concierge_missing`, `paused` or `queued_for_concierge`. A runtime reply is matched to the exact message and turn; PTY handling follows the exact bound tab's real turn events. There are no handling timers. A turn ending without an accepted linked reply becomes `no_reply`; an unconfirmed delivery becomes `failed`. A confirmed later reply still updates its incoming row. Internal session, nonce and turn correlation are not returned in history.

`neighbor-history-changed {friendship_id}` publishes handling transitions. Local status adds bounded `incoming_handling[friendship_id]` rows containing `message_id` and those handling fields, without message text. Verified messages waiting for a missing concierge can enter local history through the existing file-prefetch stream; they remain unacknowledged and are not sent to an agent. A resumed history read marks an interrupted older concierge binding as `failed` with `concierge_restarted`.

The owner or General may call `{operation:"stop_concierge",friendship_id}` through the Neighbors runtime API. It bypasses network work, interrupts only that friend's exact bound concierge through the existing stop path, and returns `{friendship_id,gateway_tab,stopped,message_ids}`. Confirmed stopped requests become `stopped` with `owner_stopped`. Stop does not send an answer or refusal to the friend. Other agents cannot call this operation.
## Pause a friend

The owner or General may call `{operation:"pause_friend",friendship_id}` or `{operation:"resume_friend",friendship_id}` on the Neighbors runtime API. Both return `{friendship_id,paused}`. Pausing blocks new messages and file grants from that friend on the server, including replies. You can still write to the friend and download files already published. Daily limits and permissions stay unchanged.

The local concierge is muted and frozen while paused; its binding and transcript remain. Already queued questions receive a polite linked refusal without invoking the concierge. Resume unmutes the same binding. Pause state follows the account to its other machines through account events and is persisted locally for startup.

Friend-list rows expose `my_paused`, its local alias `paused`, and `their_paused`. Concierge status carries `paused` and `my_paused`, with state `paused` on the receiving machine. `neighbor-friend-paused` carries `{friendship_id,paused}`. A send blocked by the recipient returns HTTP 409 with `code` and `reason` both `recipient_paused`; a paused concierge's operational calls return HTTP 403 with `neighbor_paused`.
## Setting up a friend's concierge

General asks how much the owner trusts this friend, proposes one preset, and summarizes its settings in plain words before saving. Offer to narrow access to a project with memory areas, workspace IDs and a folder. Keep machine access off unless the owner asks for it: the current providers cannot implement `none`, so leave the concierge disabled when that is required. Explain and obtain their choice before applying a preset's `read_only` or `folder` access; never silently widen it to `full`.

All presets use `allowed:true`, an owner-approved `scope`, `allow_files:true`, and `allow_interactive_content:false`. Message limits are set separately with `set_daily_limit`.

| Preset | Exact permission choices | Daily messages |
|---|---|---|
| Trusted friend | `allow_memory:true`, `memory_areas:null`, `workspaces:null`, `allow_tab_requests:true`, `machine_access:"read_only"`, `folder:null` | 100 |
| Acquaintance | `allow_memory:false`, `memory_areas:null`, `workspaces:null`, `allow_tab_requests:false`, `machine_access:"read_only"`, `folder:null` | 20 |
| Project collaborator | `allow_memory:true`, `memory_areas:["project"]`, `workspaces:["<project workspace id>"]`, `allow_tab_requests:true`, `machine_access:"folder"`, `folder:"<absolute project folder>"` | 50 |
| Custom | Ask for each grant, memory area, workspace, machine access and folder explicitly | Owner's choice |

A creation grant requires at least one workspace. A Trusted friend preset therefore still needs a workspace selection before its concierge can create agents. General resolves the selected project to its actual workspace ID and validates memory areas against the current taxonomy. Explain read-only access honestly: it permits shell commands and reads of this OS user's files; the own-pad exception permits reply files, and provider/platform enforcement may be advisory. Show the returned `folder_enforcement.note` rather than promising confinement the provider does not support.

Ordinary agent creation in a granted workspace inherits immutable friendship authority before launch, the same machine-access setting and configured working folder, and only its own AgentPad exception. Descendants retain the original concierge root. They get the same command allowlist and current grants; none can become General or edit permissions. Custom provider arguments, resumed foreign sessions, existing-tab claims, shell-only tabs, Team/plan dispatch and mesh launches stay refused. Any permission edit retires descendants, and replacing or closing the concierge closes its descendants into Recently Closed. The friendship log and saved files remain.
## Files

Attach up to four files in a conversation. Each encrypted object is at most 2 MiB, leaving 2,097,120 bytes for the original file. Text and private attachment metadata together must fit the message's 32,752-byte plaintext limit. The receiving installation must support files; otherwise sending refuses with `recipient_files_unsupported`.

Files download automatically into that friend's own folder, including while its concierge is missing or `allow_files` is off. Domios verifies the encrypted object and original bytes before saving, then confirms the saved file to the server. It sanitizes filenames for Windows reserved names, invalid characters and path traversal, and never automatically opens, executes or unpacks a file. The owner opens saved files or the folder from the friend page. Attachment keys and private manifests never appear in conversation history, notifications or concierge text.

`allow_files` defaults on and controls only the exact bound concierge's access to saved files. With that grant, the concierge may list saved files and resolve a file handle to its absolute path inside the friend's folder, then read it using its own tools. It cannot open an operating-system application, upload a local path, change limits or enumerate another friend's files through those handles. Revoking the grant refuses subsequent reads with `neighbor_files_not_allowed`. The friend's receive folder is independent of the concierge's chosen working folder.

The recipient allows 50 MiB per friend per UTC day by default and may set any byte limit from zero through 52,428,800; zero blocks new files and null restores the default. Sender caps are plan-dependent: trial 20 MiB, Pro 50 MiB, Max 200 MiB and Sponsor 500 MiB daily. The daily file-count cap is the sender's byte cap divided by 2 MiB. The server charges grants without refunds and deletes encrypted blobs after confirmation or seven days. A file refusal retains typed quota and reset details; an uncertain message submission is never automatically resent.

Use `POST /api/account/neighbors/files` with one of these JSON operations:

- `{operation:"send",friendship_id,kind,body,in_reply_to:null,paths:["absolute local path"]}` seals, uploads and finalizes each file before sending the encrypted message. The receipt includes public `attachments` entries.
- `{operation:"list",friendship_id}` returns `{folder,files}` for the owner or General. File entries include `file_id`, `message_id`, `name`, `mime`, `size_bytes`, `direction` and `state`; incoming states are `downloading`, `saved`, `failed` or `expired`.
- `{operation:"open",friendship_id,file_id}` opens a saved received file for the owner or General. For an authorized bound concierge, list and open instead return saved-file metadata with `path`; they never launch an application.
- `{operation:"open_folder",friendship_id}` opens the friend's folder for the owner or General.
- `{operation:"usage",friendship_id}` reads today's `quota.friend` and `quota.sender`. Each has `limit_bytes`, `used_bytes` and `resets_at`; sender also has `limit_files` and `used_files`. Read on opening the friend page or after sending, never on a timer.
- `{operation:"set_daily_limit",friendship_id,file_daily_limit_bytes:null}` changes the recipient's inbound limit for the owner or General. Friend-list rows expose `my_file_daily_limit_bytes` and `their_file_daily_limit_bytes`.

`neighbor-files-changed` carries `friendship_id` when the local file index changes. Join files to conversation entries by `message_id`. `neighbor-file-daily-limit-changed` carries `friendship_id` and `file_daily_limit_bytes`. Local file listing and opening do not contact the relay. Each installation keeps its own saved files; changing the receiving machine does not copy them.

Refusal replies must link to an incoming message in this friendship's local history and stay within 1024 UTF-8 bytes. `reply_target_unknown` (400) and `refusal_too_long` (413, `limit` and `actual`) are proven pre-send refusals: nothing was sent or appended to outgoing history. Keep refusals short; on `refusal_too_long`, shorten and resend once. These are not uncertain submissions.

### Applying concierge permissions

Before applying a permission edit, General must tell the owner: "Applying restarts <friend>'s concierge." Gather all field edits and submit one `set_permission` operation from the Apply button or General flow. Each apply restarts once; the permissions, scope and refusal rules become the concierge's initial system/developer instructions. Incoming turns carry only friend/message identity, kind, reply linkage, one reply instruction and body, so the scope is not repeated on every message.

The provider capability `supports_initial_prompt_update_on_resume` determines whether Apply resumes the same session with updated instructions. Claude, Codex and OpenCode support this through their existing resume instruction channels and retain agent history. Providers without that capability start a fresh concierge; the friend's local conversation history is retained. Restore uses the persisted initial instructions, and reconciles older concierge launches before accepting new turns. A failed restart is reported; it is not a successful Apply.

<!-- revision: 33 -->
