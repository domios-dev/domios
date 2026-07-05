---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_94a0131c65d44dd2a8800e0ea67434b2
handle: branch-review-r3-cli-prompt-templates-diff-vs-develop-ms_94a0131c65d44dd2a8800e0ea67434b2
title: review
---

Placeholder audit completed for src-tauri/prompts/*.md. Active placeholders found: bootstrap_instructions {general_dir} -> service/general_agent/instructions.rs render_bootstrap_md; dreaming_rem {telegram_notify_step} -> mem/dreaming/rem.rs; general_agent {identity,version,platform,hostname,soul,provider_name,general_dir,wake_up,codeterm_cli} -> service/general_agent/instructions.rs render_agent_md; mem_extraction {id,title,body,existing_areas,existing_graph_types,dedup_threshold} -> mem/auto_extract.rs; mem_rerank {query,items} -> mem/rerank.rs; plugin_agent_task_preamble {task} -> plugin/js_runtime/host_compute.rs; worker_task_preamble {intent,worker_id,task} -> mem/workers.rs; new orchestrator/planner suffix placeholders {plan_skills_principles,planner_testing_methodology} -> prompts.rs orchestrator_suffix()/planner_suffix() with tests. No CRITICAL/HIGH placeholder leak, prompt contradiction, or personal-data leak found in scoped prompt diff.
