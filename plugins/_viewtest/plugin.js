// Phase-0 fixture: a view-only plugin. Its iframe UI talks to it via the
// `ct.invoke` bridge → this single `viewCall` entry → host.* (gated).

function viewCall(method, args) {
  if (method === "echo") return { echoed: args };
  if (method === "platform") return { platform: host.platform() };
  return { error: "unknown method: " + method };
}

module.exports.default = { viewCall: viewCall };
