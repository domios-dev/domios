// Evaluate one expression in the dev renderer and print the JSON result.
// Usage: node eval.mjs --cdp 9263 --vite 1460 'expression'
import { connect, evaluate, pageTarget } from "./cdp.mjs";

const argv = process.argv.slice(2);
const arg = (name) => argv[argv.indexOf(`--${name}`) + 1];
const target = await pageTarget(Number(arg("cdp")), Number(arg("vite")));
const client = await connect(target.webSocketDebuggerUrl);
try {
  console.log(JSON.stringify(await evaluate(client, argv[argv.length - 1]), null, 1));
} finally {
  client.close();
}
