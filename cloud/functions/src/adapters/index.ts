import type { CodeStore, TokenMinter } from "../loginCodes";
import { FirestoreCodeStore, AdminTokenMinter } from "./firebase/store";
import { LocalCodeStore } from "./local/store";
import { LocalTokenMinter } from "./local/tokens";
import { FakeCodeStore, FakeTokenMinter } from "./fake";

export type AccountsProvider = "firebase" | "local" | "fake";
export interface AccountsAdapters { store: CodeStore; tokens: TokenMinter }

export function accountsAdapters(provider: "local"): {store: LocalCodeStore; tokens: LocalTokenMinter};
export function accountsAdapters(provider: "fake"): {store: FakeCodeStore; tokens: FakeTokenMinter};
export function accountsAdapters(provider: "firebase"): AccountsAdapters;
export function accountsAdapters(provider: AccountsProvider): AccountsAdapters;
export function accountsAdapters(provider: AccountsProvider): AccountsAdapters {
  switch (provider) {
    case "firebase": return {store:new FirestoreCodeStore(),tokens:new AdminTokenMinter()};
    case "local": return {store:new LocalCodeStore(),tokens:new LocalTokenMinter()};
    case "fake": return {store:new FakeCodeStore(),tokens:new FakeTokenMinter()};
    default: throw new Error("Unknown accounts adapter");
  }
}
