export const protocolVersion = 1;
export const methods = {
  initialize: "initialize",
  newSession: "session/new",
  load: "session/load",
  resume: "session/resume",
  fork: "session/fork",
  list: "session/list",
  close: "session/close",
  prompt: "session/prompt",
  steering: "_session/steering",
  cancel: "session/cancel",
  update: "session/update",
  extensionUpdate: "_x.ai/session/update",
  requestPermission: "session/request_permission",
  elicitation: "elicitation/create",
} as const;
export const clientCapabilities = {
  fs: {readTextFile: false, writeTextFile: false},
  terminal: false,
} as const;
