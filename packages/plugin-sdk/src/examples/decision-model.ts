import type { DecisionAnswer, DecisionModelCapability, PluginModule } from "../capabilities";
import type { Host } from "../host";

declare const host: Host;

const decisionModel: DecisionModelCapability = {
  primitives: () => ({ choice: true, noul: true, score: true }),
  decide(request) {
    return host.fetch.async(
      {
        url: "http://127.0.0.1:1234/decision",
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request),
      },
      (result) => {
        if (result.error || !result.body) throw new Error("decision request failed");
        const answer: unknown = JSON.parse(result.body);
        return answer as DecisionAnswer;
      },
    );
  },
};

const plugin: PluginModule = { ...decisionModel };
export default plugin;
