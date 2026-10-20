# macOS LARK input adapter notes

## Status

The app maps native events through its local-recording adapter; grants,
reservations and generation checks remain in the plugin registry and service.
The IOHID descriptor and lifecycle fixtures live in this crate. Extraction does
not establish physical LARK input or capture/transcription support; the approved
macOS element mapping and physical-device validation remain separate evidence.

## Selected receiver and element

The proposed input identifier includes the IOHID location and IORegistry entry ID. IOHIDManager matching narrows by vendor, product, and location; setup selects by exact registry identity and validates the application collection from the device's elements. VID/PID alone is not treated as receiver identity.

The collection identifier and shared declaration usage pair identify the application collection. The Mac-local selection has a separate optional approved element usage from native descriptor evidence; without it setup fails closed as `macos_hid_element_mapping_unavailable`. Setup accepts exactly one Button element with that approved element usage under the selected application collection. The element must have a binary 0..1 logical range, one report bit, one report count, and must be neither an array nor relative. The decoder treats a 0-to-1 edge as a candidate Press, ignores a repeated 1, and resets on release. The fixture's vendor-page element usage is synthetic and does not establish the physical LARK mapping or double-press semantics.

Only the selected IOHID device object and approved element can reach the decoder. The trigger uses IOHID input-value callbacks, which expose values for elements issued by interrupt-driven reports; it does not infer report byte offsets. A removed device reports `Removed`; an observed permission loss reports `AccessLost`; other native failures report `Error`. Stop invalidates callbacks, stops the dedicated `lark-macos-hid` run loop, and acknowledges only after callbacks are unregistered and the manager and retained run-loop handle are released. `MacTeardown::wait_blocking` has a five-second failure deadline, preserves its receiver and run-loop handle after timeout, and must run outside the Tauri async runtime, such as in the host adapter's blocking pool. Keep its exact generation and device reservation until a later wait returns actual cleanup success.

## Permission and troubleshooting

The implementation checks IOHID listen-event access only when manager open fails. An explicit access request is attempted only when the OS reports access as unknown; a denied or still-unknown result after the retry is reported as `AccessLost`. A manager close error is returned as cleanup failure and never as teardown acknowledgement. This conditional path does not establish that Input Monitoring is universally required for the LARK receiver; that requires runtime evidence on macOS.

Microphone consent is separate from IOHID access. Source inspection found `NSMicrophoneUsageDescription` in `src-tauri/Info.plist` and its generated bundle value in `build.rs`; this lane changed neither file. If voice capture lacks microphone access, troubleshoot the microphone permission independently of the receiver callback.

- No selected receiver or an ambiguous match: reconnect the intended receiver, select its exact input identity, and arm again.
- Device or collection identity changed: reselect the receiver and collection; reconnect does not silently rearm an old generation.
- Access denied or lost: grant the permission shown by the macOS runtime, then explicitly arm again.
- Descriptor rejected: keep the device unarmed until its observed application collection and button element are reviewed; do not substitute Windows report bytes.
- Removal or unload: the active generation ends; reconnect and rearm before another capture.

The native implementation is isolated in `src-tauri/crates/codeterm-input-macos/src/native.rs` and links IOKit and CoreFoundation. `subscribe` has a five-second setup acknowledgement deadline and returns success only after selected-device/descriptor validation, permission/open success, callback registration, and observed run-loop entry. Timeout or post-acquisition setup failure returns `MacSetupFailure` with a pending teardown handle while cleanup continues. A setup-timeout cancellation is rechecked on the native thread and at loop entry. Atlas maps setup failure to `LocalInputSetupError` and retains the reservation until actual release; Atlas calls `subscribe` through `spawn_blocking` outside locks and retains a timed-out setup task until its result can be retired. A stale generation returned after disable or unload must be retired before another subscription can use that exact device. Native property and matching dictionary keys are owned CoreFoundation strings created from Apple's documented key literals and released after use. No bundle or microphone-permission change is included. Rollback is limited to removing this macOS module, its fixtures, and this note; any later shared adapter wiring must be rolled back by its owner as a separate change.

## Required verification

Run `./build.sh check` and `./build.sh test-one input-macos lark_input_macos` in an Orchestrator-authorized warm arm64 Mac Team shell. Record the native build and fixture count separately. Then run the physical DEV oracle with the exact LARK receiver and microphone: double-press START/STOP, speak a unique token into the frozen A capture while B is focused, and exercise cancel, unplug, access denial, and unload. A mock or native compile does not replace that physical result. Intel-Mac proof also remains pending.
