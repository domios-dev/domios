# Windows LARK input

## Descriptor provenance

The live Raw Input inventory contained the selected receiver collection with path prefix `HID#VID_3547&PID_0008&MI_00&Col01`; its per-machine instance suffix is intentionally omitted. `HidP_GetCaps` succeeded for this exact collection and reported top-level usage page `0x000C`, usage `0x0001`, input report length 3, eight input button caps, zero input value caps, eight data indices, and one link collection.

`HidP_GetButtonCaps` reported eight singleton input usages on report ID `0x06`, all on page `0x000C`, link collection 0, link usage `0x0001`, and bit field `0x0042`. The descriptor maps data indices 0 through 7 to usages `0x00B7`, `0x00B1`, `0x00B6`, `0x00B5`, `0x00CD`, `0x00E2`, `0x00EA`, and `0x00E9`, respectively. The selected report states supplied with the task were parsed against this live preparsed descriptor by `HidP_GetUsages`: `06 01 00` had active usage `0x00E9`; `06 00 00` had no active usages. The implementation only accepts this exact caps fingerprint and derives the active usage through HID parser APIs; it does not treat report bytes as a usage.

The report pair and its one-pair-per-physical-double-press cardinality are supplied state evidence. This worker did not produce a new physical capture. Descriptor parsing is observed on this machine; physical button behavior remains a separate DEV step below.

## Selection and lifecycle

The adapter requires the full selected Raw Input path and `Col01`. VID/PID, MI_00, and collection identity are checked before descriptor validation, and runtime reports must come from the exact selected Raw Input device handle. Windows registers only the Consumer Control top-level usage needed by Col01; a foreign handle is discarded before its report payload is parsed or logged. No keyboard usage is registered and no report data is logged.

Raw Input permits one target window per device class in a process. The adapter uses a process-local single-listener claim and checks existing registrations before it registers Consumer Control; if another owner is present, setup fails without replacing it. See [RegisterRawInputDevices](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices).

Descriptor/report counts, preparsed allocation, report length, raw packet size, report batch count, and button usage count are bounded. A changed descriptor fails closed. A valid rising `0x000C:0x00E9` state emits one generation-bound press; repeated active state and the release state are inert. A short monotonic duplicate window suppresses rapid repeated transitions, with no second-press wait.

Removal invalidates the generation and emits `Removed`; access denial emits `AccessLost`; other native failures emit `Error`. Removal ends the native listener. A newly attached handle is not rearmed automatically: the host must create a fresh subscription for the newly selected exact path, which revalidates the descriptor before setup can return success. No press is synthesized.

Stop posts a quit request to the dedicated listener thread. Cleanup unregisters the Raw Input usage, destroys the message window, drops preparsed data, then sends the completion result. `wait_blocking` also joins the listener thread; call it from a blocking worker, after releasing host locks, never from the Tauri async runtime. A cleanup error is not a successful teardown acknowledgement.

`WindowsInputSubscription::start_pending` only reports thread creation. The adapter must call `WindowsInputPendingSubscription::wait_until_ready` inside `spawn_blocking`, outside registry/host locks, before returning success from `LocalInputAdapter::subscribe`. Readiness arrives only after exact path and descriptor validation, Raw Input registration, and entry into the message pump; there is no native-only `Armed` callback. Setup failure returns `WindowsInputSetupFailure` with its teardown handle so the exact device/generation reservation remains until cleanup completes. If generation validation makes a successful setup obsolete, request stop and retain its teardown until acknowledged.

`request_stop` captures a failed thread wake. `WindowsInputTeardown::wait_blocking` has a bounded completion and thread-exit deadline and must run off the runtime and locks. Timeout preserves the mutable teardown object for a later wait; it is not acknowledgement. `LISTENER_CLAIMED` remains set until native release succeeds, so dropping a timed-out registry entry cannot permit conflicting rearm. A successful acknowledgement can carry a wake error if completion independently proves cleanup; an unregister/window-destroy error never acknowledges release.

## Selection troubleshooting

- If the exact selected path is absent, reselect the receiver's live `MI_00 Col01` collection. Do not substitute another collection or a VID/PID-only match.
- If the top-level usage, input report length, button-cap set, report ID, link collection, or data-index map changes, the subscription reports an error and stays inert. Capture a new descriptor inventory before changing the accepted schema.
- Unplug reports `Removed`; access denial reports `AccessLost`. Reconnect requires explicit host rearm and a new generation. It cannot synthesize a press.
- MI_07 code 28 is outside this adapter and remains untouched.

## Focused Windows DEV button step

Owner selects the approved receiver's exact full path and Col01, records `HidP_GetCaps` and `HidP_GetButtonCaps`, arms the selected subscription, and performs one physical double-press. Record the listener generation and require exactly one `Press` for the descriptor-resolved `0x000C:0x00E9` rising state; its release must emit none. Verify unrelated input produces no decoded/logged report, then unplug and reconnect to confirm removal invalidates the generation and a fresh explicit subscription emits no synthetic press. This step proves the button adapter only; it does not claim recording or transcription.

Warm fixture selector: `./build.ps1 test-one app lark_input_windows`. It must execute all 16 deterministic fixtures with a nonzero case count. No native test is ignored; the physical DEV step is not part of the fixture selector.

## Transmitter battery

The selected receiver has a sibling Col03 vendor collection on page0xFF90 with64-byte input/output reports and input reportID0x55. A fixed heartbeat status request55 AA DD10 40 00 00 27 returns the receiver response55 BB DD10 80, a big-endian payload length, and two connection flags followed by two battery percentages. A read on the connected0008 receiver observed slot1 off and slot2 at97percent. Validate the exact same physical receiver instance, descriptor, response framing and0–100 values; unsupported or unavailable replies show no percentage. No settings or firmware command is exposed. Queries run off-runtime with bounded completion deadlines and cancelled IO ownership on failure.

Protocol references: https://github.com/soradev64/omarchy-lark-battery and https://github.com/blchinezu/larkctl.
