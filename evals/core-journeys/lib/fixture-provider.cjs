const settings = __FIXTURE_SETTINGS__;
const valid = id => /^[a-f0-9-]{36}$/i.test(id);
function command(id) {
  if (!valid(id)) return "";
  return settings.command + " --session '" + id + "'";
}
module.exports.default = {
  // The host's readiness probe calls this without a session id; the real launch injects a fresh one.
  buildLaunchCommand: params => params.sessionId == null ? settings.command : command(params.sessionId),
  buildResumeCommand: id => command(id),
  buildResumeCommandWithContext: params => command(params.sessionId),
  sessionResume: () => ({ exact: true }),
  sessionFilePath: (_cwd, id) => valid(id) ? settings.sessions + "/" + id + ".jsonl" : null,
  sessionExists: (_cwd, id) => valid(id) && host.fs.fileExists(settings.sessions + "/" + id + ".jsonl"),
  screenHasTui: () => false,
  detectEvents: () => [],
  parsePrompt: () => null,
  starterPromptText: () => undefined,
};
