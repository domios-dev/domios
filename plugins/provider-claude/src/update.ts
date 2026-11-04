
import { checkProviderUpdate } from "../../provider-shared/updateCheck";

export function* checkClaudeUpdate() {
  return (yield* checkProviderUpdate(host, "claude", "https://registry.npmjs.org/@anthropic-ai%2Fclaude-code/latest"));
}
