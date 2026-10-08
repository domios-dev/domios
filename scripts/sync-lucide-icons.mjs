import { writeFileSync } from "node:fs";
import dynamicIconImports from "lucide-react/dynamicIconImports.mjs";

writeFileSync(
  new URL("../src/lib/lucideIconNames.json", import.meta.url),
  `${JSON.stringify(Object.keys(dynamicIconImports).sort())}\n`,
);
