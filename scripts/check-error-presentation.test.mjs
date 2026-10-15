import assert from "node:assert/strict";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { checkErrorPresentation, fieldValidation, rawErrorRenders } from "./check-error-presentation.mjs";

const file = "src/components/Fixture.tsx";
test("flags direct error strings, members, templates and String conversion", () => {
  for (const expression of ["error", "error.message", "String(error)", "failure.reason", '`Failed: ${error}`']) {
    assert.equal(rawErrorRenders(`const view = <p>{${expression}}</p>`, file).length, 1, expression);
  }
});
test("follows raw errors stored under another state name, without treating event parameters as errors", () => {
  const source = 'const [note, setNote] = useState(""); try { work(); } catch (problem) { setNote(String(problem)); } const view = <p>{note}</p>';
  assert.equal(rawErrorRenders(source, file).length, 1);
  assert.equal(rawErrorRenders('const [query, setQuery] = useState(""); const view = <><input onChange={e => setQuery(e.target.value)} /><p>{query}</p></>', file).length, 0);
});
test("flags failure diagnostics renamed to message inside an alert", () => {
  assert.equal(rawErrorRenders('const view = <p role="alert">{state.message}</p>', file).length, 1);
});
test("does not flag notice props, event handlers, error predicates, metrics or rendered node bindings", () => {
  const source = 'const inputFailureLine = failed ? <InlineErrorNotice error={error} /> : null; const view = <>{error && <InlineErrorNotice error={error} />}<button onClick={() => console.error(error)}>{isError ? "Retry" : "Done"}</button>{inputFailureLine}<p>{summary.errors} errors</p></>';
  assert.deepEqual(rawErrorRenders(source, file), []);
});
test("field validation exemptions are exact expressions, not whole-file exemptions", () => {
  const field = "src/components/shared/FormRow.tsx";
  assert.equal(rawErrorRenders('const view = <p>{error}</p>', field).length, 0);
  assert.equal(rawErrorRenders('const view = <p>{error.message}</p>', field).length, 1);
  assert.equal(fieldValidation.size, 4);
});
test("raw JSX error strings cannot be added to the component tree", () => {
  const root = fileURLToPath(new URL("../", import.meta.url));
  assert.deepEqual(checkErrorPresentation(root), []);
});
