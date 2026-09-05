import { refusalText } from "./refusalText";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function run() {
  const fallback = "Could not add team member.";

  // codeterm#418: Tauri rejects with a bare string, and the coded refusal used
  // to be replaced by a retry prompt for a refusal that never becomes possible.
  assert(
    refusalText(
      "team_orchestrator_already_live: Team team_x already has a live Orchestrator (O1)",
      fallback,
    ).startsWith("team_orchestrator_already_live"),
    "a string rejection keeps its structural code",
  );
  assert(
    refusalText(new Error("team_caller_unauthorized: Caller tab W1 is not a member"), fallback)
      .startsWith("team_caller_unauthorized"),
    "an Error rejection keeps its structural code",
  );
  assert(refusalText("   ", fallback) === fallback, "a blank string falls back");
  assert(refusalText(new Error(" "), fallback) === fallback, "a blank Error message falls back");
  assert(refusalText(undefined, fallback) === fallback, "an unknown rejection falls back");
  assert(!fallback.includes("Try again"), "a structural refusal never invites a retry");

  console.log("refusal text tests passed");
}

run();
