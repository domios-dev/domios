# Trusted plugin view callbacks

A view document is registered by the authenticated local host loader, not by its
iframe. The loader resolves a complete surface/instance/tab tuple against the live
mux, including its owner, workspace and Team. An empty tuple is explicitly an
unowned panel; a partial, foreign or stale tuple refuses. Panels have no inferred
focused workspace and cannot admit owned jobs without a separate host scope path.

The existing bounded frame store owns provider identity, expiry, revocation and
media handles. Every view now registers there, including views without media.
Each registration has a private backend-call lease and a distinct presentation
media key. Playback URLs already expose the latter, so it cannot authorize view
callbacks. Neither is used as the context's public frame identity (a fresh UUID).
The private lease is sent only in an authenticated response header, closed over
by the host document loader and supplied outside iframe-controlled arguments.

Native ViewProvider::call and JS viewCall(method,args,context) receive the same
ViewCallContext: plugin, explicit origin, nullable surface/instance/tab/owner/workspace/team,
actor {kind:local}, and non-secret frame. UI calls cannot claim an agent actor;
onSurfaceCall's agent actor remains a separate entry point. Result lookup must
use the trusted context's scope, not an owner/job scope echoed in args. The
registered provider is captured before callback entry; lease/provider and mux
scope are revalidated after await. Refusals occur before dispatch for stale
registrations; late results never reach a different document, including reloads
that retain the same WindowProxy. Suppressing a stale UI result does not roll
back or discard a backend's durable receipt.

A Rust-only invocation projection is scoped around the blocking JS callback.
Future job admission reads this projection, rechecks context_is_live plus live
mux scope and persists its own operation before effects. No JavaScript setter,
raw lease, synthetic run, budget or monitor authority is exposed. Native trusted
providers receive the same context directly. Unowned panel projections have no
owner_scope. The internal invalidate_plugin_owner_views port accepts a persisted
host ViewOwnerScope, derives opted-in live registered instances, and returns the
number invalidated; zero is normal. It does not create a monitor or polling loop.

Media lookup uses this identical context before minting a delivery handle; Courier
owns the mediaFile context integration. Frame TTL/handle limits remain bounded by
the existing media store. Frontend disposal revokes the whole registration. This
change adds no filesystem, secret or network grant and no runnable paid pipeline.

Two existing host UI clients do not render an iframe. They use narrow Local-only
ports instead of a missing-frame exception. Remote task read/refresh/submit checks
the live tab's ChatBackend provider/session and saved execution mode against the
registered remoteTask capability; expected provider/session claims cannot redirect
or survive a session change. Session id passed to the callback comes from the
backing, not args. Recording suggestion checks that plugin's declaration/grant and
generation, calls only selectRecordingDevices with bounded inventory hints, and
still leaves device selection to the existing validated selection command. These
contexts explicitly say remote_task or recording_setup, with no frame. The generic
iframe callback route continues to require its private document lease. None of
these host ports is callable through ct.invoke as an arbitrary dispatch target.

The invocation snapshot remains on the same blocking thread while the existing
call_json bridge releases its VM and resumes explicit host.awaitJob continuations.
This does not pump arbitrary Promises or grant authority through a saved object:
new admissions/effects must recheck the live registration and host scope. Durable
job callbacks later use their ledger scope, never revive an expired frame context.

Document invocation and consent metadata/grants both use the originating base
transport; a selected mesh relay never retargets either side of the callback.
The existing recording hint callback is selectRecordingDevices, exposed only by
its fixed host suggestion port, not a caller-selected export. UI supplies expected
remote provider/session claims; the host compares them to current backing before
any callback, so an old UI cannot submit into a replacement session.

The private lease is never part of a URL, iframe HTML or public context. Disposal
posts a bounded strict JSON body to a fixed Local-only route, so URI diagnostics
cannot record it. The response also carries a distinct public document UUID.
Before exposing srcdoc, the loader registers that UUID with a host channel broker.
The trusted first bootstrap transfers one MessagePort; its paired endpoint stays
in a closure, with a captured native postMessage function. Plugin requests, host
gestures, media lookup, replies, support and invalidations all use this port.
Window messages accept only that initial port transfer, never a call or reply.
Every iframe load stops frontend dispatch, deletes the backend registration and
all its media handles, then waits for retirement acknowledgement before challenging
the original private port. A valid proof registers a fresh lease/context before
dispatch resumes. Old TLS contexts never regain admission, even after proof. Navigation before the first load cannot inherit the endpoint,
even with the same WindowProxy; a second hello refuses and requires explicit reopen.
Initial reads may run after the first proof without waiting for load, avoiding
module-await deadlock. Every incoming envelope is checked before direct or queued dispatch (256 KiB,
bounded JSON depth/nodes; malformed values refuse). The proof queue separately
allows 32 packets and 256 KiB in total; gesture
intent and generation are captured at receipt before any queue or await. No timer
or automatic document replay is used. A closed frame cannot revoke a durable job
receipt already committed by the plugin; only stale presentation is suppressed.

Re-registration keeps only the presentation namespace for the original document's
fixed media CSP; handles and backend identity are always fresh. A Local-only bounded
request carries the host-captured original surface/owner/workspace/Team and provider
registration id. Moved scopes, replaced providers and occupied namespaces refuse.
The generic capability registry's host-only registration id fences replacement
without exposing pointers or deserializing a ViewCallContext supplied by a frame.
Retirement and registration are serialized by the loader; stale challenge results
cannot dispatch. A failed retirement or registration closes the frontend port.
The backend deletion acknowledgement is the retirement linearization boundary;
effects already admitted before it retain their durable receipts. No suspension
flag, old-context reactivation, timer or generic frame-less callback was added.
