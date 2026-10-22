const { execFileSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const path = require("node:path");

function devConnection(options = {}, run = execFileSync) {
  const gitBash = "C:/Program Files/Git/bin/bash.exe";
  const bash = process.platform === "win32" && existsSync(gitBash) ? gitBash : "bash";
  const env = { ...process.env };
  const directory = options.dataDir && path.resolve(options.dataDir);
  let instance = options.instance;
  if (instance === undefined && directory) {
    const name = path.basename(directory);
    if (!/^\.domios-dev(?:-.+)?$/.test(name)) throw new Error("Expected a DEV data directory");
    instance = name === ".domios-dev" ? "" : name.slice(".domios-dev-".length);
  }
  if (instance !== undefined) env.CODETERM_INSTANCE = String(instance);
  else if (env.CODETERM_DEV_INSTANCE) env.CODETERM_INSTANCE = env.CODETERM_DEV_INSTANCE;
  if (options.apiPort !== undefined) env.CODETERM_API_PORT = String(options.apiPort);
  else if (env.CODETERM_DEV_API_PORT) env.CODETERM_API_PORT = env.CODETERM_DEV_API_PORT;
  const output = run(bash, ["-c",
    `source "$1"; dev_run python3 -c 'import json,os; print(json.dumps({"api":os.environ["CODETERM_API_URL"],"token":os.environ["CODETERM_API_TOKEN"],"dataDir":os.environ["CODETERM_DATA_DIR"]}))'`,
    "dev-connection", path.join(__dirname, "lib.sh")], { env, encoding: "utf8", windowsHide: true });
  const connection = JSON.parse(output);
  const normalize = (value) => process.platform === "win32" ? path.resolve(value).toLowerCase() : path.resolve(value);
  if (directory && normalize(connection.dataDir) !== normalize(directory)) {
    throw new Error("DEV credential resolved to a different data directory");
  }
  if (!connection.token?.trim() || /[\r\n]/.test(connection.token)) throw new Error("Missing or invalid DEV credential");
  return connection;
}

module.exports = { devConnection };
