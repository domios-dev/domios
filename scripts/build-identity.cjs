const { execFileSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const path = require("node:path");

function gitExecutable(env = process.env, platform = process.platform) {
  if (platform !== "win32") return "git";
  const candidates = [env.ProgramFiles, env["ProgramFiles(x86)"]]
    .filter(Boolean)
    .map((programFiles) => path.join(programFiles, "Git", "cmd", "git.exe"));
  return candidates.find((candidate) => existsSync(candidate)) ?? "git";
}

function gitOutput(args, options = {}) {
  const env = options.env ?? process.env;
  const output = (options.execFileSync ?? execFileSync)(gitExecutable(env, options.platform), args, {
    cwd: options.cwd,
    encoding: "utf8",
    env,
  }).trim();
  if (!output && !options.allowEmpty) throw new Error(`git ${args.join(" ")} returned no output`);
  return output;
}

function requireBuildIdentity(value, source) {
  if (typeof value !== "string" || !value.trim() || value.trim() === "unknown") {
    throw new Error(`${source} must provide a non-unknown build identity`);
  }
  return value.trim();
}

function resolveBuildIdentity(options = {}) {
  const env = options.env ?? process.env;
  if (env.CODETERM_FRONTEND_BUILD_HASH !== undefined) {
    return requireBuildIdentity(env.CODETERM_FRONTEND_BUILD_HASH, "CODETERM_FRONTEND_BUILD_HASH");
  }
  const identity = requireBuildIdentity(
    (options.gitOutput ?? gitOutput)(["rev-parse", "--short", "HEAD"], options),
    "git rev-parse --short HEAD",
  );
  if (!/^[0-9a-f]{7,40}$/.test(identity)) {
    throw new Error("git rev-parse --short HEAD returned an invalid build identity");
  }
  return identity;
}

module.exports = { gitExecutable, gitOutput, requireBuildIdentity, resolveBuildIdentity };
