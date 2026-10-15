export function condition(expression, { needs = {}, inputs = {}, vars = {}, github = {}, failed = false, cancelled = false, steps = {} } = {}) {
  const source = expression.replace(/^\$\{\{ | \}\}$/g, "")
    .replaceAll(/(needs|steps)\.([\w-]+)/g, (_, scope, id) => `${scope}[${JSON.stringify(id)}]`);
  const startsWith = (value, prefix) => String(value ?? "").toLowerCase().startsWith(String(prefix).toLowerCase());
  return Boolean(Function("needs", "steps", "inputs", "vars", "github", "startsWith", "failure", "cancelled", "always", `return (${source})`)(needs, steps, inputs, vars, github, startsWith, () => failed, () => cancelled, () => true));
}
