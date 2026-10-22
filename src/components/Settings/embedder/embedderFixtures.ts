import type { EmbedderListing, EmbedderStatus } from "./embedderModel";

export const statusFixture = (overrides: Partial<EmbedderStatus> = {}): EmbedderStatus => ({
  provider: "local",
  model: "l12",
  dimension: 384,
  remote: false,
  available: true,
  index: { provider: "local", model: "l12", dimension: 384 },
  index_matches: true,
  vectors: 10,
  pending: 0,
  reembed: { state: "idle", target: null, total: 0, done: 0, failed: 0 },
  ...overrides,
});

export const listingFixture = (refused: boolean): EmbedderListing => ({
  providers: [
    { id: "local", label: "Local", remote: false, active: true, refusal: null,
      models: [{ model: "l12", dimension: 384, min_dimension: 384, max_tokens: 512, default: true }] },
    { id: "remote-x", label: "Remote", remote: true, active: false,
      refusal: refused ? { message: "no key stored", hints: ["cat > key"] } : null,
      models: [{ model: "wide", dimension: 1536, min_dimension: 1, max_tokens: 8191, default: true }] },
  ],
  remote_opt_in: !refused,
  remote_warning: "memory text leaves the machine",
  status: statusFixture(),
});
