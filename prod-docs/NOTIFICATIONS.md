# Notification configuration

Levels, per-level surfaces, sound, and push channels. Everything defaults to the
bell alone; every other surface is opt-in and asks the OS for permission when it
is switched on.

```toml
[notifications]
desktop = false     # opt-in; the per-level grid below still decides which levels toast
sound = false       # the chime's master mute; each level below says whether it chimes
toast_duration = 5  # seconds
show_bell = true    # false removes the indicator; every record is still kept

# Where each level may go BEYOND the panel. Every notification is kept in the
# panel whatever is written here. Any combination of columns is legal, including
# none. `bell` is inert; `show_bell` controls the indicator; a row's `sound`
# selects that level's chime (default: `action_needed` and `failure` chime) and
# `notifications.sound` is the master mute. Settings offers one channel picker per
# level; unpicking a channel removes its key, which reads as closed.
[[notifications.agent_levels]]
level = "verbose"
bell = true
desktop = false
[[notifications.agent_levels]]
level = "action_needed"
bell = true
desktop = false
# sound = false        # omit to keep the chime that shipped for this level
# telegram = true      # one flag per registered push channel, keyed by service id
[[notifications.agent_levels]]
level = "failure"
bell = true
desktop = false

`desktop` is the one local surface a row routes to. Every other key on a row is
a push channel's service id, and a channel key that is absent reads as closed —
a column nobody named is a column nobody chose, so a channel the registry gains
arrives off and stays off until someone opts into it. The same answer serves the
shipped default, which is why a config written before the grid existed now reads
as interrupting nowhere rather than as whatever the surrounding switches allowed
then.

`bell` on a row is **retained and inert**. It is written above because the field
still exists in the stored shape, and it is deliberately not removed: the channel
keys are flattened onto the row, so a row that stopped naming `bell` would read
a stored `bell = true` back as a push channel called `bell` that no registry will
ever resolve. Nothing routes on it, settings does not offer it, and changing it
changes nothing. It used to gate the panel's only live event, which meant a
`bell = false` level was recorded while the panel silently lagged until the next
mount — not "this level does not reach the bell", which is why the column is gone.

`show_bell` is the bell answer config still makes, and it is presentation:
`show_bell = false` removes the indicator from the titlebar while every
notification is still recorded, so turning it back on reveals the complete log.
Because it takes the panel with it, it is also the one switch that can leave a
record with no surface at all: with the indicator hidden, no desktop toast
admitted and no push channel carrying it, `codeterm notify send` says so under
the id it returns, naming which surface is shut and why. The record is still
kept and the exit code is still `0` — that is information, not a failure.

Everything under `notifications` is set from Settings, not from `codeterm
config set`. Where a notification may go is your standing answer, so an agent
that could switch a desktop toast or a push channel on by itself would grant
itself a capability you never saw. A `codeterm config set notifications.*` — and
any config write arriving from inside a tab that carries a `notifications`
section — is refused with `403`. An agent may OFFER instead: `codeterm notify
send "…" --choice "Turn it on" --choice "Not now" --offer desktop --offer-accept
"Turn it on"` sends a question, and the surface changes only when you tap the
accepting label. An untapped or declined offer changes nothing. The refusal is
not agent-specific — the daemon cannot tell you from an agent inside a tab, so
the same command typed by hand in a CodeTerm tab is refused too.

A Needs you notification can point at one chat message: `codeterm notify send
"…" --level action_needed --target-tab <tab id> --chat-message <message id>`. On
the Mission Control Board its row is a single jump that leaves the Board, opens
that tab in Chat and reveals that exact message, never a similar one. Only a
provider-uuid message id is stable enough to jump to; a position-based id such
as `0-601-0` reads as having no stable id. When no message was recorded, the tab
is closed or has no chat view, or the message is not in the loaded chat, the row
says it is unavailable instead. Both ids are navigation only and never change who asked.

# Per-channel push AUDIENCE, keyed by the channel's bridge service id — the same
# id `codeterm bridge send --service <id>` takes. Whether a level reaches a
# channel at all is that channel's column of the level grid above; this answers
# only who receives what the grid routed here. A channel with no entry has no
# audience, and an empty audience is nobody: it never falls back to that
# integration's allowed-users list, which admits inbound senders instead.
# Settings offers those admitted ids as the choices, and stores only what you pick.
[notifications.channels.telegram]
recipients = []        # explicit audience (chat ids) — NOT telegram_allowed_users
per_scope_limit = 12   # lifetime push ceiling per originating tab (a count, not a rate)
```

## Lifecycle and validation

`notify send` refuses a blank title, a title over 200 characters (put detail in
the pad body), and a `--target-tab` that names no open tab on this machine.

Pending prompt detection updates tab state but never creates a notification.
Agents that need owner attention must explicitly run
`codeterm notify send "<title>" --level action_needed`. CodeTerm still raises process-exit
and recovery-stopped notices for tabs that need a restart; each notice belongs to
one standing condition of one tab and resolves when that condition ends or the
tab closes. Hidden automation routes prompts and exits to a live report owner,
and only writes them to the CodeTerm log when it has none. A cron run with a
pending prompt remains awaiting its owner; prompt detection itself does not
notify the user.

Unread sticky notices — the condition notices above, and any record created with
`sticky` over the API — survive a restart of CodeTerm, kept in
`notifications.json` in the data folder; one whose tab did not come back resolves
at startup. Read, resolved and non-sticky records do not persist, and neither
does an offer to turn a surface on, which must be sent again.

## Related documents

The rest of the configuration file: `codeterm docs CONFIG`. Bridge and integration
settings: `codeterm docs INTEGRATIONS`.

<!-- revision: 9 -->
