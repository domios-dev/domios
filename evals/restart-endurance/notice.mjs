import { spawn } from "node:child_process";

const child = spawn(process.env.ENDURE_BASH ?? "bash", process.argv.slice(2), {
  stdio: "inherit", signal: AbortSignal.timeout(30_000),
});
child.on("error", (error) => {
  console.error(`Restart notice refused or exceeded 30 seconds: ${error.message}`);
  process.exit(2);
});
child.on("close", (code) => { process.exitCode = code === 0 ? 0 : 2; });
