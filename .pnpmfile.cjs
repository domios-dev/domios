const fs = require('node:fs');
const path = require('node:path');

function refuseLinkedDirectory(directory, root) {
  const relative = path.relative(root, directory);
  if (!relative || relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`pnpm refused dependency directory outside this checkout: ${directory}`);
  }
  let current = directory;
  while (current !== root) {
    try {
      if (fs.lstatSync(current).isSymbolicLink()) {
        throw new Error(`pnpm refused linked dependency directory: ${current}. Unlink it without recursion and install inside this checkout.`);
      }
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    current = path.dirname(current);
  }
}

module.exports = {
  hooks: {
    updateConfig(config) {
      const root = path.resolve(config.lockfileDir || __dirname);
      const modules = path.resolve(root, config.modulesDir || 'node_modules');
      refuseLinkedDirectory(modules, root);
      refuseLinkedDirectory(path.resolve(root, config.virtualStoreDir || path.join(modules, '.pnpm')), root);
      return { ...config, packageImportMethod: 'copy' };
    },
  },
};
