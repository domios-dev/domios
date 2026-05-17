// Fresh Codex provider plugin — written from scratch as an outside
// developer would, following only the published manifest schema +
// host API surface. Intentionally lives alongside the bundled
// `provider-codex` so we can compare and confirm the architecture
// supports independent third-party authoring with zero Rust changes.
//
// The id is `codex-fresh` so it doesn't shadow the bundled one in
// the registry.

module.exports.default = {
  detectFromTitle: function (title) {
    return /codex/i.test(String(title || ""));
  },

  detectFromOutput: function (text) {
    var s = String(text || "");
    return (
      s.indexOf("OpenAI Codex") !== -1 ||
      s.indexOf("codex>") !== -1 ||
      s.indexOf("permissions: YOLO") !== -1
    );
  },

  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = ["codex"];
    if (p.headless && p.task) {
      parts = ["codex", "exec"];
      if (p.skipPermissions) {
        parts.push("--dangerously-bypass-approvals-and-sandbox");
      }
      parts.push("--skip-git-repo-check");
      parts.push(host.shell.quote(p.task));
    } else if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["codex", "resume"];
    if (skipPermissions) parts.push("--dangerously-bypass-approvals-and-sandbox");
    parts.push(host.shell.quote(sessionId));
    return parts.join(" ");
  },

  detectEvents: function (text) {
    var s = String(text || "");
    if (
      s.indexOf("Would you like to run") !== -1 ||
      s.indexOf("approve network") !== -1 ||
      s.indexOf("grant these permissions") !== -1
    ) {
      return ["permission_request"];
    }
    return [];
  },
};
