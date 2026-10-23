import { useState } from "react";
import type { BlueprintPort } from "../../lib/blueprintApi";

export function BlueprintParameters({ inputs }: { inputs: Readonly<Record<string, BlueprintPort>> }) {
  const [values, setValues] = useState<Record<string, string>>({});
  return <section aria-label="Input parameters">
    {Object.entries(inputs).map(([name, port]) => <label key={name}>{name} ({port.type ?? port.$ref ?? "JSON"})<input aria-label={`Parameter ${name}`} value={values[name] ?? ""} onChange={(event) => setValues((current) => ({ ...current, [name]: event.target.value }))} /></label>)}
    <output aria-label="CLI parameter assignments">{Object.entries(values).map(([name, value]) => <code key={name}>--set {JSON.stringify(`${name}=${value}`)} </code>)}</output>
  </section>;
}
