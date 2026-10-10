import { decodeCliMessageText } from "./messageText";

const runtimePath = "C:\\Users\\U\\.codeterm-dev\\runtime\\x.md";
if (decodeCliMessageText(runtimePath) !== runtimePath) throw new Error("Windows runtime path escapes must stay verbatim");

const escapedPath = "C:\\new\\tab";
if (decodeCliMessageText(escapedPath) !== escapedPath) throw new Error("Windows newline and tab path escapes must stay verbatim");

const proseLines = "one\\ntwo\\r\\nthree";
if (decodeCliMessageText(proseLines) !== "one\ntwo\nthree") throw new Error("CLI prose line breaks must decode");

const loneCarriageReturn = "one\\rtwo";
if (decodeCliMessageText(loneCarriageReturn) !== loneCarriageReturn) throw new Error("lone CLI carriage-return escape must stay verbatim");

const uncPath = "\\\\server\\share\\new\\tab";
if (decodeCliMessageText(uncPath) !== uncPath) throw new Error("UNC path escapes must stay verbatim");
