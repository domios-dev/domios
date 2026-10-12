import { once } from "node:events";
import { expect, it } from "vitest";
import { localServer } from "../src/adapters/local/server";
it("completes local checkout and cancellation with persisted entitlement reads", async () => {
  const server = localServer();
  server.listen(0,"127.0.0.1");
  await once(server,"listening");
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing server address");
  const base = `http://127.0.0.1:${address.port}`;
  let session = "";
  async function post(path: string, data: unknown) {
    const response = await fetch(`${base}/api/${path}`,{method:"POST",headers:{"Content-Type":"application/json",Authorization:`Bearer ${session}`},body:JSON.stringify(data)});
    expect(response.status).toBe(200);
    return response.json();
  }
  try {
    session = (await post("signin",{})).session;
    const checkout = await post("createCheckoutSession",{plan:"pro"});
    expect(checkout.url.startsWith(base)).toBe(true);
    expect((await fetch(checkout.url)).status).toBe(200);
    await post("localPayment",{status:"active"});
    expect(await post("entitlement",{})).toMatchObject({plan:"pro",status:"active"});
    const portal = await post("createPortalSession",{});
    expect((await fetch(portal.url)).status).toBe(200);
    await post("localPayment",{status:"canceled"});
    expect(await post("entitlement",{})).toMatchObject({plan:"pro",status:"canceled"});
  } finally { await new Promise<void>((resolve,reject) => server.close(error => error ? reject(error) : resolve())); }
});
