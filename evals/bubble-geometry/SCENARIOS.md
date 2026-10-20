# Scenarios

| Browser viewport | Observable pass condition |
| --- | --- |
| 1440 × 1000 | Available transcript width exceeds the token cap; rows and assistant columns equal the cap and are centered within gutters. User bubbles fit their contents within that frame, aligned to its left edge. |
| 1920 × 1000 | Same width and centering; widening the pane does not widen readable content. |
| 2560 × 1000 | Same width and centering at an ultrawide viewport. |
| 390 × 1000 | Rows and assistant columns fill the available pane width minus its measured gutters and scrollbar. User bubbles fit their contents without exceeding that width. |

All four require a visible composer bounded by the shared token, loaded user and assistant messages, and exact viewport acknowledgement. The oracle rejects an intentionally oversized copied row in every scenario. No delays, live agents or message delivery are involved.
