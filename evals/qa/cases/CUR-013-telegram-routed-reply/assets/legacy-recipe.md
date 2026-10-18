# Telegram inbound and routed reply

Purpose: prove the real far-end chat/thread receives the intended response once. Core only under an owner-authorized real bridge grant; unit injection is not a substitute.

## Preconditions / ownership

Owner supplies exact test bot/chat/thread/recipient, sender, allowed message body, test target tab and cleanup permission. Read TELEGRAM-SETUP/ROUTING through the target instance CLI and current bridge help. A host credential cannot enter a DEV bench. Keep token in the authorized secret store/process memory. Never print it or use argv, pads or screenshots. One getUpdates poller per token; do not start another to test reachability.

Keel's handoff explicitly withheld bot transfer/prod mutation and reserved main-chat production smoke for the owner. That historical permission does not authorize a future tester to send. If no dedicated test integration is available, mark NOT RUN and request the exact owner scope; do not borrow the owner's main conversation.

## Steps / far-end evidence

1. Owner records active bridge/poller identity and current test thread/recipient. Tester pins the daemon build/boot and owned target/session; capture routing state without tokens. Discover `dc bridge ... --help` only for DEV. A separately authorized prod smoke uses host codeterm outside bench machinery with an explicit prod boundary and no lifecycle action.
2. Owner sends a single unique inbound marker from the real test Telegram thread. Require the target's own-session routed user turn once, correct sender/chat/thread provenance and expected reply instruction. A received webhook/HTTP200 or echo in a log does not prove agent delivery.
3. Let the target answer through its host-supplied bridge route. Bodies go through a pad file with the current supported --from-pad contract. Never invent a route or send to a guessed recipient. Record exact outbound message ID/target/thread and delivered state. Do not replay input if delivery is uncertain.
4. Inspect the actual remote Telegram thread and require the final answer once with correct context. Preserve a screenshot with personal data redacted and actual message IDs. Verify DOM/API counts in the target own session as supporting evidence. A bridge ACK alone cannot pass.
5. Extended cases (long append, attachment, topic close) need exact grants; observe actual remote content/ordering and recipient. Closing topic-owned tabs may delete Telegram topics, so retain them unless deletion is explicitly authorized.

PASS: real inbound once and real remote reply once in the intended thread. FAIL: wrong target/thread, duplicate/lost message or false delivery. NOT RUN: missing bot/thread/poller grant, unavailable far-end view, human consent or missing supported test endpoint. A real bridge smoke cannot be passed by invoking an invented inject route.

Cleanup only own message/topic/tab fixtures with the owner's exact deletion grant. Do not rotate tokens, stop the owner poller, delete main threads or change General's provider. Preserve uncertain sends for classification.

Seeds: Keel's critical subset deliberately excluded public bridge injection because no route existed. Canonical red/green proved wiring but real bot proof remained owner-run. The Turing summary does not establish Telegram acceptance; no positive live result is imported here.

Automation: pure route/provenance/once-delivery mechanisms can be unit-tested. One controlled far-end real-thread check remains essential. A scoped simulator can test wiring, clearly labeled separate from Telegram delivery.
