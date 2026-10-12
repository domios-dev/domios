import type { CodeStore, LoginCode } from "../../loginCodes";

export class LocalCodeStore implements CodeStore {
  protected readonly codes = new Map<string, LoginCode>();
  private readonly rates = new Map<string, {count:number; expires_at:number}>();

  async insert(hash: string, record: LoginCode): Promise<void> {
    if (this.codes.has(hash)) throw new Error("Duplicate code");
    this.codes.set(hash, {...record});
  }

  async consume(hash: string, validate: (record: LoginCode | undefined) => string): Promise<string> {
    const record = this.codes.get(hash);
    const uid = validate(record ? {...record} : undefined);
    this.codes.set(hash, {...record!,used:true});
    return uid;
  }

  async allowAttempt(ipHash: string, now: number, limit: number, windowMs: number): Promise<boolean> {
    const current = this.rates.get(ipHash);
    const active = current && current.expires_at > now;
    const count = active ? current.count : 0;
    if (count >= limit) return false;
    this.rates.set(ipHash, {count:count+1,expires_at:active ? current.expires_at : now+windowMs});
    return true;
  }
}
