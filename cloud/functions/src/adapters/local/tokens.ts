import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import type { TokenMinter } from "../../loginCodes";

export class LocalTokenMinter implements TokenMinter {
  private readonly key = randomBytes(32);
  constructor(private readonly now: () => number = Date.now) {}
  async mint(uid: string): Promise<string> {
    const body = Buffer.from(JSON.stringify({uid,exp:this.now()+3_600_000})).toString("base64url");
    return `${body}.${this.signature(body)}`;
  }
  verify(token: string): string {
    const parts = token.split(".");
    if (parts.length !== 2) throw new Error("Invalid token");
    const expected = Buffer.from(this.signature(parts[0]));
    const actual = Buffer.from(parts[1]);
    if (expected.length !== actual.length || !timingSafeEqual(expected,actual)) throw new Error("Invalid token");
    const body = JSON.parse(Buffer.from(parts[0],"base64url").toString()) as {uid:string;exp:number};
    if (typeof body.uid !== "string" || !Number.isFinite(body.exp) || body.exp <= this.now()) throw new Error("Invalid token");
    return body.uid;
  }
  private signature(body: string): string { return createHmac("sha256",this.key).update(body).digest("base64url"); }
}
