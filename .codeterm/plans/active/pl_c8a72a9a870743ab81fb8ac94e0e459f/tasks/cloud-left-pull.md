---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_8264a233e0ff4968a1872f4e7581629a
handle: cloud-left-pull
title: 'Cloud: in_progress/idle pull left + subtasks drag along'
status: done
---

# worlds-v3 cloud: in_progress/idle pull to LEFT edge; subtasks drag with parent
Andrey: in the task cloud, tasks that are in_progress OR idle (not done) should pull toward the LEFT edge of the cloud (closer to the plan / the "active" side), while done sink away. Tasks WITH subtasks must DRAG their subtask-swarm ALONG when they move left (the whole super-cell + its satellite children move together). Refine layoutTaskCloud / the two-zone sunflower: bias active/in_progress/idle toward the left (the eye/plan side) instead of a generic center core; keep the subtask mini-cloud attached to its parent through the move. Deterministic, §31 (no camera move), no jitter. Re-open of worlds-v3 (task_5f44bce3). Verify: active tasks cluster left with their subtasks; done drift right/away.
