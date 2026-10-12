import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { parseChallenge, type DesktopChallenge } from "../../shared/protocol";

export const LOGIN_ERROR = "Unable to complete desktop sign-in";
export const CODE_LIFETIME_MS = 120_000;
export const RATE_LIMIT = 30;

export interface LoginCode extends DesktopChallenge {
  uid: string;
  expires_at: number;
  used: boolean;
}

export interface CodeStore {
  insert(hash: string, record: LoginCode): Promise<void>;
  consume(hash: string, validate: (record: LoginCode | undefined) => string): Promise<string>;
  allowAttempt(ipHash: string, now: number, limit: number, windowMs: number): Promise<boolean>;
}

export interface TokenMinter {
  mint(uid: string): Promise<string>;
}

export function sha256(value: string): string {
  return createHash("sha256").update(value).digest("hex");
}

interface Redemption extends DesktopChallenge { code: string }

function parseRedemption(input: unknown): Redemption {
  if (typeof input !== "object" || input === null) throw new Error(LOGIN_ERROR);
  const data = input as Record<string, unknown>;
  if (typeof data.code !== "string" || (data.code.length !== 43 || /[^A-Za-z0-9_-]/.test(data.code))
    || typeof data.code_verifier !== "string" || (data.code_verifier.length < 43 || data.code_verifier.length > 128 || /[^A-Za-z0-9._~-]/.test(data.code_verifier))) {
    throw new Error(LOGIN_ERROR);
  }
  const code_challenge = createHash("sha256").update(data.code_verifier).digest("base64url");
  return { ...parseChallenge({...data, code_challenge}), code: data.code };
}

export class CodeExchange {
  constructor(private readonly store: CodeStore, private readonly tokens: TokenMinter, private readonly now: () => number = Date.now) {}

  async create(uid: string, input: unknown): Promise<string> {
    const challenge = parseChallenge(input);
    if (!uid) throw new Error(LOGIN_ERROR);
    const code = randomBytes(32).toString("base64url");
    await this.store.insert(sha256(code), {uid, ...challenge, expires_at:this.now()+CODE_LIFETIME_MS,used:false});
    return code;
  }

  async allowAttempt(ip: string): Promise<boolean> {
    return this.store.allowAttempt(sha256(ip), this.now(), RATE_LIMIT, 60_000);
  }

  async redeem(input: unknown): Promise<string> {
    const request = parseRedemption(input);
    const uid = await this.store.consume(sha256(request.code), (record) => {
      if (!record || record.used !== false || !Number.isFinite(record.expires_at) || record.expires_at <= this.now()
        || record.state !== request.state || record.redirect_uri !== request.redirect_uri
        || typeof record.code_challenge !== "string" || typeof record.uid !== "string" || !record.uid) throw new Error(LOGIN_ERROR);
      const expected = Buffer.from(record.code_challenge);
      const actual = Buffer.from(request.code_challenge);
      if (expected.length !== actual.length || !timingSafeEqual(expected, actual)) throw new Error(LOGIN_ERROR);
      return record.uid;
    });
    return this.tokens.mint(uid);
  }
}
