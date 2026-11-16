import path from "node:path";
import { API } from "typescript/unstable/sync";
import { createVirtualFileSystem } from "typescript/unstable/fs";

export function parseTypeScriptSources(sources) {
  const directory = path.resolve(".domios-source-parser").replaceAll("\\", "/");
  const config = `${directory}/tsconfig.json`;
  const files = [...sources].map(([name, text], index) => ({ name, text, file: `${directory}/${index}${name.endsWith("sx") ? ".tsx" : ".ts"}` }));
  const fs = createVirtualFileSystem(Object.fromEntries([
    [config, JSON.stringify({ compilerOptions: { noLib: true, noResolve: true, types: [], jsx: "preserve" }, files: files.map(row => row.file) })],
    ...files.map(row => [row.file, row.text]),
  ]));
  const api = new API({ fs });
  try {
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    try {
      const project = snapshot.getProject(config);
      if (!project) throw new Error("TypeScript source parser project was not loaded");
      return new Map(files.map(({ name, file }) => {
        const source = project.program.getSourceFile(file);
        if (!source) throw new Error(`TypeScript did not parse ${name}`);
        return [name, source];
      }));
    } finally {
      snapshot.dispose();
    }
  } finally {
    api.close();
  }
}

export function parseTypeScriptSource(file, source) {
  return parseTypeScriptSources(new Map([[file, source]])).get(file);
}
