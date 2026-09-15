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

# Where each level may go BEYOND the panel. The panel is the complete log and
# keeps every record whatever is written here — there is no column for it, and
# no way to route a level away from it. Rows are generated from the same closed
# level set the push channels use, and are reconciled on read: a level added to
# that set appears here on its own, and a row naming no known level is dropped.
# Any combination of columns is legal, including none — that level is kept in
# the panel and interrupts nowhere. A fresh install ships what is written below,
# which interrupts nowhere at every level. An agent notification that names no
# level is kept too, and never reaches the system or a push channel.
# `bell` is RETAINED AND INERT. Nothing routes on it and settings does not offer
# it; it stays a named field because the channel keys are flattened into the
# row, so dropping it would make a stored `bell = true` read back as a push
# channel called "bell" that no registry will ever resolve. `notifications.
# show_bell` is a different field and still works: it hides the indicator.
# `sound` on a ROW is that level's own chime answer, and is a named field for the
# same reason `bell` is: a `sound` key in the flattened map would read back as a
# push channel called "sound". Omit it and the level keeps the chime that shipped
# — `action_needed` and `failure` chime, `verbose` does not — so a config written
# before the field sounds exactly as it did. Write it and the answer wins, in
# both directions. `notifications.sound` above is the master mute over all of
# them; it is not a destination and cannot widen where a level goes.
# This used to be argued the other way: a per-level chime was refused because it
# would allow a level that reaches nothing yet still makes a noise, with the noise
# as its only trace. The bell gate has since been retired and EVERY level reaches
# the panel unconditionally, so no such configuration exists any more and the
# objection is gone.
# Settings offers the system notification as a toggle and the push channels as
# ONE picker per level, so a registered channel widens that picker instead of
# adding a row. Only a channel the daemon reports as able to carry a message is
# offered. Unpicking a channel REMOVES its key from the row rather than writing
# `false`; both read as closed, which is what an unnamed channel has always been.
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

## Related documents

The rest of the configuration file: `codeterm docs CONFIG`. Bridge and integration
settings: `codeterm docs INTEGRATIONS`.

<!-- revision: 5 -->
