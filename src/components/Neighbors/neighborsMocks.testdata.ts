import { vi } from "vitest";

export function createNeighborMocks() {
const notices = ({ publishNotice: vi.fn() });
const transport = ({ localFetch: vi.fn() });
const bridge = ({ handlers: {} as Record<string, Set<(event: { payload: unknown }) => void>>, invoke: vi.fn(async () => undefined) });
const account = ({ model: { kind: "signed_in", identity: { uid: "owner", email: "me@example.test", display_name: "Me", photo_url: null } } as Record<string, unknown>, signIn: vi.fn() });
const mux = ({
  workspaces: [{ id: "ws-general", name: "General", tabs: ["tab-general"], protected: true }, { id: "ws-work", name: "Work", tabs: ["tab-gateway"] }],
  tabs: { "tab-general": { id: "tab-general", title: "Main", protected: true }, "tab-gateway": { id: "tab-gateway", title: "Concierge" } },
  tabStatuses: {},
});

return { notices, transport, bridge, account, mux };
}
