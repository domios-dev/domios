import type { LoginCode, TokenMinter } from "../../loginCodes";
import { LocalCodeStore } from "../local/store";

export class FakeCodeStore extends LocalCodeStore {
  record(hash: string): LoginCode | undefined { const value = this.codes.get(hash); return value ? {...value} : undefined; }
  seed(hash: string, record: LoginCode): void { this.codes.set(hash,{...record}); }
}

export class FakeTokenMinter implements TokenMinter {
  readonly minted: string[] = [];
  async mint(uid: string): Promise<string> { this.minted.push(uid); return `test-token:${uid}`; }
}
