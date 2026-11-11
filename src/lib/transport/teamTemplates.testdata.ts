import preview from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/preview.json";
import startRequest from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/start-request.json";
import startResults from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/start-results.json";
import modelDecisions from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/model-decisions.json";
import recall from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/recall.json";
import list from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/list.json";
import unavailable from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/unavailable.json";
import type {
  TemplateList,
  TemplatePreview,
  TemplateRecall,
  TemplateStartRequest,
  TemplateStartResult,
  TemplateUnavailable,
} from "./teamTemplates";

type ModelDecision = import("../../../gen/ModelDecision").ModelDecision;

// The same JSON a Rust test round-trips through the contract types, so UI fixtures cannot drift.
export const templatePreviewFixture = preview as TemplatePreview;
export const templateStartRequestFixture = startRequest as TemplateStartRequest;
export const templateStartResultFixtures = startResults as TemplateStartResult[];
export const modelDecisionFixtures = modelDecisions as ModelDecision[];
export const templateRecallFixture = recall as TemplateRecall;
export const templateListFixture = list as TemplateList;
export const templateUnavailableFixture = unavailable as TemplateUnavailable;
