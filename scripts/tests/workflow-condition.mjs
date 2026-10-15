export function condition(expression, { needs = {}, inputs = {}, vars = {}, failed = false, cancelled = false, steps = {} } = {}) {
  const source = expression.replace(/^\$\{\{ | \}\}$/g, "")
    .replaceAll(/(needs|steps)\.([\w-]+)/g, (_, scope, id) => `${scope}[${JSON.stringify(id)}]`);
  return Boolean(Function("needs", "steps", "inputs", "vars", "failure", "cancelled", "always", `return (${source})`)(needs, steps, inputs, vars, () => failed, () => cancelled, () => true));
}
