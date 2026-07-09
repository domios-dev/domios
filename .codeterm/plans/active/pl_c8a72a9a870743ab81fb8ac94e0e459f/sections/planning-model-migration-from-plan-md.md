---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_4ad8487989a744a3b673cbe895601e1e
handle: planning-model-migration-from-plan-md
title: Planning model migration from plan.md
---

Integrate the model migration from /Users/rollacode/Desktop/plan.md into CodeTerm itself, not into a side markdown plan.

Execution rules:
- CodeTerm planner remains the source of truth.
- Product prompts/docs teach raw agents that they can become orchestrators and must read codeterm docs ORCHESTRATION when promoted.
- The model should be strict: planning work lives in Goal/Spec/Plan sections; execution Tasks are generated only after an approval gate.
- Discovery/research is an intermediate wave, not completion. Discovery tasks produce evidence, gaps, decisions, and follow-up requirements; the orchestrator must autonomously synthesize the remaining implementation/review/verification task graph and continue until the plan GOAL is complete.
- No legacy compatibility layer for this personal release; breaking planning schema changes are acceptable for 1.6.2.
- Superpowers planning habits are imported as quality checks, not as markdown storage.
- Every remaining gap from plan.md becomes a planner task with acceptance and gates.

Verification map:
- cargo test --manifest-path cli/Cargo.toml
- cargo test --manifest-path src-tauri/Cargo.toml prompts -- --nocapture
- cargo check --manifest-path src-tauri/Cargo.toml
- CODETERM_API_PORT=7686 ./build.sh check
- bash scripts/check-file-sizes.sh --fail
- npm run check:versions
