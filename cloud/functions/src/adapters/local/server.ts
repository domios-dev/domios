import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { CodeExchange, LOGIN_ERROR } from "../../loginCodes";
import { accountsAdapters } from "..";
import { BillingService } from "../../billing/service";
import { LocalGateway } from "../../billing/adapters/local";
import { MemoryBillingStore } from "../../billing/adapters/memory";

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
  const billingStore = new MemoryBillingStore();
  const billingGateway = new LocalGateway(randomBytes(32).toString("hex"));
  const billing = new BillingService(billingGateway,billingStore,{mode:"test",plans:{pro:"local_pro"},hostedOrigin:"https://local.domios.invalid"});
  const json = (response: ServerResponse, status: number, value: unknown) => {
    response.writeHead(status,{"Content-Type":"application/json","Cache-Control":"no-store","Referrer-Policy":"no-referrer"});
    response.end(JSON.stringify(value));
  };
  return createServer((request,response) => {
    void (async () => {
      const host = request.headers.host;
      if (!host || !/^127\.0\.0\.1:[1-9][0-9]{0,4}$/.test(host)) throw new Error(LOGIN_ERROR);
      if (request.headers.origin && request.headers.origin !== `http://${host}`) throw new Error(LOGIN_ERROR);
      billingGateway.setOrigin(`http://${host}`);
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
        if (path === "/api/createCheckoutSession" && signedIn) return json(response,200,await billing.checkout("local-user",body));
        if (path === "/api/createPortalSession" && signedIn) return json(response,200,await billing.portal("local-user"));
        if (path === "/api/entitlement" && signedIn) return json(response,200,billingStore.entitlements.get("local-user") ?? null);
        if (path === "/api/localPayment" && signedIn) {
          const customer = await billingStore.customer("local-user");
          if (!customer || !body || typeof body !== "object" || !("status" in body)
            || !["active","canceled"].includes(String(body.status))) throw new Error("Invalid local payment");
          const event = Buffer.from(JSON.stringify({id:randomBytes(16).toString("hex"),mode:"test",created:Math.floor(Date.now()/1000),customer,
            change:{plan:"pro",status:body.status,current_period_end:Math.floor(Date.now()/1000)+2592000,cancel_at_period_end:false}}));
          await billing.webhook(event,billingGateway.sign(event));
          return json(response,200,billingStore.entitlements.get("local-user"));
        }
        if (path === "/api/createDesktopLoginCode" && signedIn) return json(response,200,{code:await exchange.create("local-user",body)});
        if (path === "/api/signout" && signedIn) { sessions.delete(session); return json(response,200,{}); }
        if (path === "/api/redeemDesktopLoginCode") return json(response,200,{custom_token:await exchange.redeem(body)});
        if (path === "/api/account" && typeof body === "object" && body !== null && "custom_token" in body && typeof body.custom_token === "string") {
          return json(response,200,{uid:tokens.verify(body.custom_token)});
        }
        throw new Error(LOGIN_ERROR);
      }
      if (request.method !== "GET") throw new Error(LOGIN_ERROR);
      if (path === "/billing" || path === "/billing/checkout" || path === "/billing/portal") {
        response.writeHead(200,{"Content-Type":"text/html","Cache-Control":"no-store"});
        response.end(`<!doctype html><html lang="en"><meta charset="utf-8"><title>Local billing</title>
        <h1>Local billing simulator</h1><p>No payment details needed.</p>
        <button id="pay">Activate Pro</button><button id="cancel">Cancel plan</button><p role="status" id="status">Ready</p>
        <script>
        let session;
        async function post(path,data) {
          const response = await fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json',...(session?{Authorization:'Bearer '+session}:{})},body:JSON.stringify(data)});
          if (!response.ok) throw new Error('Billing request failed');
          return response.json();
        }
        async function change(status) {
          try {
            if (!session) session = (await post('signin',{})).session;
            await post('createCheckoutSession',{plan:'pro'});
            await post('localPayment',{status});
            const entitlement = await post('entitlement',{});
            document.getElementById('status').textContent = 'Pro: '+entitlement.status;
          } catch { document.getElementById('status').textContent = 'Billing unavailable'; }
        }
        document.getElementById('pay').onclick=()=>change('active');
        document.getElementById('cancel').onclick=()=>change('canceled');
        </script></html>`);
        return;
      }
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
