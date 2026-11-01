# Identical chat send evidence

`observe-status.mjs` observes an already-open event stream for two owned sends.
Open the stream before sending, retain both POST receipts, then pass the exact
tab id and distinct send ids. It consumes `chat-send-status` payloads directly;
unrelated events never trigger ledger reads. The caller's event stream must
honor the supplied observation signal, as `Events` in core-journeys does.

Inject a `readLedger(signal)` function that reads the tab's chat-send endpoint
under that signal, and a `persist(receipt)` function that writes the receipt
immediately. The observer reads the ledger initially and finally only. The
final read has an independent ten-second failure deadline, so an expired
observation still retains the final response or an explicit read error.

The completion oracle requires delivered statuses and distinct matched source
rows for every send in the final ledger. Delivery alone is insufficient.
After completion the live driver must verify two rendered user rows, reload
its own page, and retain a separate post-reload ledger snapshot showing the
same claims. No uncertain send is replayed. All live access uses the DEV bench
lib; this module does not contact a daemon itself.
