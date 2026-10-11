import type { TransportClient } from "../transportClient";

export function orchestrationTestClient(json: TransportClient["json"], subscribe: TransportClient["subscribe"] = () => () => undefined): TransportClient {
  return {
    json,
    localJson: (method, path, body, opts) => json(method as "GET", path, body, opts),
    upload: async () => { throw new Error("Unexpected fixture upload"); },
    subscribe,
    currentToken: () => "fixture-token",
    origin: () => "http://fixture.test",
  };
}
