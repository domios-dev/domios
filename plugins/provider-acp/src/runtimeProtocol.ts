export const protocolVersion = 1;
export const methods = {
  initialize: "initialize",
  newSession: "session/new",
  load: "session/load",
  resume: "session/resume",
  prompt: "session/prompt",
  steering: "_session/steering",
  cancel: "session/cancel",
  update: "session/update",
} as const;
export const clientCapabilities = {
  fs: {readTextFile: false, writeTextFile: false},
  terminal: false,
} as const;
