export type WhenContext = Readonly<Record<string, boolean | string | number | null | undefined>>;
interface Value { value: WhenContext[string]; known: boolean }
const tokenPattern = /\s*(===|!==|&&|\|\||==|!=|[!()]|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|-?\d+(?:\.\d+)?|[A-Za-z_][\w.:-]*)/y;

export function evaluateWhen(expr: string | null | undefined, context: WhenContext): boolean {
  if (!expr?.trim()) return true;
  if (expr.length > 4096) return false;
  const pattern = new RegExp(tokenPattern);
  const tokens: string[] = [];
  let offset = 0;
  while (offset < expr.trimEnd().length) {
    pattern.lastIndex = offset;
    const match = pattern.exec(expr);
    if (!match || tokens.length >= 512) return false;
    tokens.push(match[1]);
    offset = pattern.lastIndex;
  }
  let index = 0;
  let depth = 0;
  const boolean = (value: boolean): Value => ({ value, known: true });
  const truth = (entry: Value) => entry.known && Boolean(entry.value);
  const atom = (literal = false): Value => {
    const token = tokens[index++];
    if (!token) throw new Error("Missing operand");
    if (token === "true" || token === "false") return boolean(token === "true");
    if (token === "null") return { value: null, known: true };
    if (/^-?\d/.test(token)) return { value: Number(token), known: true };
    if (token.startsWith('"') || token.startsWith("'")) return { value: token.slice(1, -1).replace(/\\(.)/g, "$1"), known: true };
    if (!/^[A-Za-z_][\w.:-]*$/.test(token)) throw new Error("Invalid operand");
    if (literal) return { value: token, known: true };
    return { value: context[token], known: Object.prototype.hasOwnProperty.call(context, token) && context[token] !== undefined };
  };
  const unary = (): Value => {
    if (++depth > 32) throw new Error("Expression too deep");
    try {
      if (tokens[index] === "!") { index++; return boolean(!truth(unary())); }
      if (tokens[index] === "(") {
        index++;
        const result = or();
        if (tokens[index++] !== ")") throw new Error("Unclosed group");
        return result;
      }
      return atom();
    } finally { depth--; }
  };
  const comparison = (): Value => {
    const left = unary();
    const op = tokens[index];
    if (!["==", "!=", "===", "!=="].includes(op)) return left;
    index++;
    const right = atom(true);
    if (!left.known || !right.known) return boolean(false);
    const equal = left.value === right.value;
    return boolean(op === "==" || op === "===" ? equal : !equal);
  };
  const and = (): Value => {
    let value = comparison();
    while (tokens[index] === "&&") { index++; const right = comparison(); value = boolean(truth(value) && truth(right)); }
    return value;
  };
  const or = (): Value => {
    let value = and();
    while (tokens[index] === "||") { index++; const right = and(); value = boolean(truth(value) || truth(right)); }
    return value;
  };
  try { const result = or(); return index === tokens.length && truth(result); }
  catch { return false; }
}
