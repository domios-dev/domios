//! REM phase: prepare candidate data for the dreaming agent and spawn the
//! headless REM worker (dedup, classify, insights, diary).

use crate::MemManager;

/// Prepare data for the REM agent — returns a text summary of candidates.
/// Called by `domios mem dream --report` so the agent gets concrete data.
pub fn prepare_rem_data(mem: &MemManager) -> Result<String, String> {
    let blueprint_merges = mem.dream_blueprints()?;
    mem.store.rem_snapshot(blueprint_merges)
}

pub const DREAMING_TELEGRAM_STEP_ENABLED: &str = r#"Then push the INSIGHT (only the insight prose, not the log block) to
telegram so the vault talks back. OK if it errors — the diary in mem
is the durable artifact:

    domios mem search "telegram chat id" --limit 1
    # extract the chat id, then:
    domios bridge send --to <chat_id> --service telegram \
      "🧠 Dreamer brief\n\n<just the insight, no <insight> tags, no log>"
    # If no fact found or bridge errors, skip — telegram is nice-to-have."#;

pub const DREAMING_TELEGRAM_STEP_DISABLED: &str = "Telegram notification is DISABLED \
    (memory.dreaming_telegram_notify=false). Skip any outbound notification — \
    do not search for chat ids, do not invoke the bridge command. The diary \
    in mem is the artifact; the user has opted out of the nightly Telegram ping.";
