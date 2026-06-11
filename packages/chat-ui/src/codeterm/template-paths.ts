// Subcommand paths the card has hand-written templates for. recognize.test.ts
// asserts each exists in docs/cli-manifest.json so a template can't point at a
// renamed/removed command.
export const TEMPLATE_PATHS: string[][] = [
  ["send"],
  ["send-text"],
  ["agent", "spawn"],
  ["agent", "report"],
  ["agent", "providers"],
  ["mem", "save"],
  ["mem", "search"],
  ["mem", "get"],
  ["workspace", "create"],
  ["pane", "focus"],
  ["mesh", "send"],
  ["cron", "create"],
  ["keepawake", "start"],
];
