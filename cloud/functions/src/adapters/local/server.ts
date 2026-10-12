import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { CodeExchange, LOGIN_ERROR } from "../../loginCodes";
import { accountsAdapters } from "..";

async function jsonBody(request: IncomingMessage): Promise<unknown> {
  if (!request.headers["content-type"]?.startsWith("application/json")) throw new Error(LOGIN_ERROR);
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.from(chunk);
    size += buffer.length;
    if (size > 4096) throw new Error(LOGIN_ERROR);
    chunks.push(buffer);
  }
  return JSON.parse(Buffer.concat(chunks).toString());
}

export function localServer(hostingDirectory = resolve(__dirname,"../../../../../../hosting/signin")) {
  const {store,tokens} = accountsAdapters("local");
  const exchange = new CodeExchange(store,tokens);
  const sessions = new Map<string,number>();
  const json = (response: ServerResponse, status: number, value: unknown) => {
    response.writeHead(status,{"Content-Type":"application/json","Cache-Control":"no-store","Referrer-Policy":"no-referrer"});
    response.end(JSON.stringify(value));
  };
  return createServer((request,response) => {
    void (async () => {
      const host = request.headers.host;
      if (!host || !/^127\.0\.0\.1:[1-9][0-9]{0,4}$/.test(host)) throw new Error(LOGIN_ERROR);
      if (request.headers.origin && request.headers.origin !== `http://${host}`) throw new Error(LOGIN_ERROR);
      const path = new URL(request.url ?? "/",`http://${host}`).pathname;
      if (path.startsWith("/api/")) {
        if (!await exchange.allowAttempt(request.socket.remoteAddress ?? "127.0.0.1")) throw new Error(LOGIN_ERROR);
        if (request.method !== "POST") throw new Error(LOGIN_ERROR);
        const body = await jsonBody(request);
        if (path === "/api/signin") {
          const session = randomBytes(32).toString("base64url");
          sessions.set(session,Date.now()+600_000);
          return json(response,200,{session});
        }
        const session = request.headers.authorization?.replace(/^Bearer /,"");
        const signedIn = session && (sessions.get(session) ?? 0) > Date.now();
        if (path === "/api/createDesktopLoginCode" && signedIn) return json(response,200,{code:await exchange.create("local-user",body)});
        if (path === "/api/signout" && signedIn) { sessions.delete(session); return json(response,200,{}); }
        if (path === "/api/redeemDesktopLoginCode") return json(response,200,{custom_token:await exchange.redeem(body)});
        if (path === "/api/account" && typeof body === "object" && body !== null && "custom_token" in body && typeof body.custom_token === "string") {
          return json(response,200,{uid:tokens.verify(body.custom_token)});
        }
        throw new Error(LOGIN_ERROR);
      }
      if (request.method !== "GET") throw new Error(LOGIN_ERROR);
      if (path === "/signin/config.json") return json(response,200,{provider:"local"});
      const file = new Map([
        ["/signin/",["index.html","text/html"]], ["/signin/index.html",["index.html","text/html"]],
        ["/signin/app.js",["app.js","text/javascript"]], ["/signin/style.css",["style.css","text/css"]],
      ]).get(path);
      if (!file) throw new Error(LOGIN_ERROR);
      const contents = await readFile(resolve(hostingDirectory,file[0]));
      response.writeHead(200,{"Content-Type":file[1],"Cache-Control":"no-store","Referrer-Policy":"no-referrer","X-Content-Type-Options":"nosniff"});
      response.end(contents);
    })().catch(() => json(response,400,{error:LOGIN_ERROR}));
  });
}

if (require.main === module) {
  const port = Number(process.env.DOMIOS_ACCOUNTS_PORT ?? "15100");
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid local accounts port");
  localServer().listen(port,"127.0.0.1",() => {
    process.stdout.write(`Local accounts ready at http://127.0.0.1:${port}/signin/\n`);
  });
}
