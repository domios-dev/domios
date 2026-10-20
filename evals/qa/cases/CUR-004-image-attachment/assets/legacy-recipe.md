# Image attachment journey

Purpose: prove actual image bytes reach the intended model once while preserving text and draft behavior. Core: PNG/JPEG, pending upload and bounded upload-failure recovery.

## Preconditions / fixtures

Shared DEV setup; an owned agent with image capability and an owned Browser. Create tiny PNG/JPEG files containing a randomly chosen visual fact (for example a white circle on a solid color), record their paths, MIME types, bytes and digests. Do not reuse an image whose answer appears in the prompt. Owner authorizes phone emulation, clipboard/picker use and any injected HTTP fault separately. A model without vision cannot pass this journey by accepting a chip.

## Steps / expected far end

1. Open own DEV chat, snapshot and scope composer to the owned tab. Paste/drop one owned PNG. Require no page navigation and one attachment chip. Repeat with JPEG in a new owned turn; no global first-composer selector.
2. In the SAME Domios Browser CDP session, use `Emulation.setDeviceMetricsOverride` with width390,height844,mobile=true and a recorded deviceScaleFactor. Verify innerWidth and visualViewport.width both390 before the phone drop. Keep the connection open through the observation; a transient connection previously yielded stale1578px evidence.
3. Submit immediately while upload is pending. Require the documented draft/sending behavior: composer clears, next draft editable, one sending bubble and one eventual payload. Read the own-session transcript with `dc tab chat --tab "$TAB" --json`: image present once, text once, intended final assistant describes the withheld visual fact. Inspect the actual image message and answer in the visible Browser DOM. HTTP200 or chip creation alone fails the oracle.
4. With a separately authorized, tab-scoped fault fixture, fail exactly the owned upload with503. Require quiet inline notice, no blocking popup and no lost text. Before Retry, inspect whether text/image already reached the own session. Retry only the failed image attachment after proven upload failure; require image once and original text still once. Never resend an uncertain chat payload.
5. Try HEIC only with a fixture and verify a visible supported refusal or actual supported result; record which. Use native picker/paste on desktop and physical iOS Photos only with the relevant device/permission. Emulated Chromium is NOT physical iOS evidence.

Capture actual native multipart request metadata (boundary/byte count/file MIME, no auth) and redacted response on failure. For API diagnosis use the registered upload route/schema from API-CHAT/current frontend; never guess `/attachments` is the same wire endpoint as native multipart. Preserve the smallest opaque wire body and first400 before assigning frontend/backend blame.

PASS needs exact own-session image/text counts plus model-visible fact and DOM. FAIL:400 on valid native payload, duplicate send, navigation, text loss or misleading retry. NOT RUN: no valid upload, vision, device or fault grant. Keep each MIME/input/subcase independent.

Cleanup owned files/chips/drafts and fixture tabs only after evidence; never delete another tester's failed draft. Release CDP overrides by clearing device metrics on the same owned Browser, then close it.

Seeds: Keel's valid PNG native multipart400 was a live FAIL awaiting a backend repair. Turing later reports PNG/JPEG bytes and model vision PASS, pending submit PASS, phone390 drop PASS, forced503/image-only Retry PASS. Physical iOS and HEIC remained NOT RUN. Historical repair does not replace a fresh release run.

Automation: pure attachment state/retry decisions belong in module tests; a bounded native FormData roundtrip can verify multipart wiring; keep one real model visual-fact check. Do not promote screenshot or serialization-only tests to image delivery proof.
