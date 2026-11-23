# Mobile flow narration

Five six-second portrait takes, followed in order by `mob-flow-reel` (30 seconds). The camera glides upward through each tall canvas with a small continuous push. All lettering belongs to real product components; there is no editorial title layer. The lower 500 pixels stay empty for voice captions.

| Scene | Duration | Picture and motion | Proposed Hale line | Truth source |
| --- | --- | --- | --- | --- |
| `mob-flow-request` | 6 s | The full real chat view at 3.4x fills the portrait band with its header, user bubble, reply and composer; the camera glides through it. | I just tell the lead what I want to get working. | Research section 1, fact 15; `essences/vision.md`, Meta-teams; real `ChatMessageRow` and `MessageBubble`. |
| `mob-flow-team` | 6 s | The actual Team sidebar fills the band from the start; three worker activity dots wake while the camera glides down. | The planner breaks it up, and the lead brings in the workers. | Research section 1, fact 15; `domios docs ORCHESTRATION`, Durable model and Dispatch contract. |
| `mob-flow-lane` | 6 s | The actual task detail shows Mira, her role, and `lane/offline-sync-resolver`; the camera travels from role to branch and description. | Each worker gets a task and a separate branch to work in. | Research section 1, fact 16; `essences/capabilities.md`, isolated-lanes and integration; real `DetailContent`. |
| `mob-flow-report` | 6 s | Mira's report fills an 885px-wide card below her worker and branch detail; the camera follows the review evidence. | When they're ready, the result comes back here for review. | Research section 1, fact 17; `domios docs ORCHESTRATION`, review distinguishes submission from acceptance. |
| `mob-flow-merge` | 6 s | Worker and branch detail, the report card, and the lead status fill the canvas; acceptance and the merge confirmation settle near mid-frame. | The lead checks it, accepts it, and merges the branch back in. | Research section 1, facts 16–17; `domios docs ORCHESTRATION`, real merge integration and owner review. |

The offline-sync example and its 214-test report are illustrative product fixtures, adapted from the existing Fieldnote fixture, rather than a claim about this film's production history. A merge is represented by the lead's chat confirmation, not an invented Git interface. No worker dispatches another worker. The film harness seeks each frame; scene timing and camera positions depend only on `t`.
