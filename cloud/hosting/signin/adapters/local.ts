import type { AuthClient } from "../authClient";

export function localClient(): AuthClient {
  if (location.hostname !== "127.0.0.1" || location.protocol !== "http:") throw new Error("Local sign-in requires loopback");
  let session: string | undefined;
  async function post(path: string, data: unknown): Promise<Record<string,string>> {
    const response = await fetch(`/api/${path}`, {method:"POST",headers:{"Content-Type":"application/json",...(session ? {Authorization:`Bearer ${session}`} : {})},body:JSON.stringify(data)});
    if (!response.ok) throw new Error("Local request failed");
    return response.json() as Promise<Record<string,string>>;
  }
  return {
    methods:["local"], isEmailLink:() => false,
    async signIn() { session = (await post("signin",{})).session; },
    async sendEmail() { throw new Error("Use the local sign-in button"); },
    async finishEmail() { throw new Error("Use the local sign-in button"); },
    async createCode(challenge) { return (await post("createDesktopLoginCode",challenge)).code; },
    async signOut() { if (session) await post("signout",{}); session = undefined; },
  };
}
