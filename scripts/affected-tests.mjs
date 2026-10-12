import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { discoverTests, selectRelatedTests } from "./run-ts-unit-tests.mjs";

const scriptRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const scopeAliases = new Map([
  ["codeterm", "app"],
  ["codeterm-analytics", "analytics"],
  ["codeterm-notification-core", "notification-core"],
  ["codeterm-account", "account"],
  ["codeterm-billing", "billing"],
  ["codeterm-cli", "cli"],
  ["codeterm-plan", "plan"],
  ["codeterm-frontend-bundle", "frontend-bundle"],
  ["codeterm-pty", "pty"],
  ["codeterm-plugin", "plugin"],
  ["codeterm-plugins", "plugins"],
  ["codeterm-types", "types"],
  ["codeterm-mem", "mem"],
  ["codeterm-mesh", "mesh"],
]);

function parseArgs(argv) {
  const options = {
    base: process.env.AFFECTED_TESTS_BASE || "origin/master",
    repo: scriptRoot,
    rustMode: "",
    files: [],
    explicitBase: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--base" || arg === "--repo" || arg === "--rust-mode") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
      if (arg === "--base") {
        options.base = value;
        options.explicitBase = true;
      } else if (arg === "--repo") {
        options.repo = path.resolve(value);
      } else {
        options.rustMode = value;
      }
      index += 1;
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      options.help = true;
      continue;
    }
    if (arg === "--") continue;
    if (arg.startsWith("--")) throw new Error(`unknown option: ${arg}`);
    options.files.push(arg);
  }
  if (options.rustMode && !["whole-crate", "module-filters"].includes(options.rustMode)) {
    throw new Error("--rust-mode must be whole-crate or module-filters");
  }
  return options;
}

function git(repoRoot, args) {
  return execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
}

function changedFiles(options) {
  if (options.files.length > 0) return options.files.map((file) => normalizeRepoPath(file, options.repo));
  const base = options.explicitBase
    ? options.base
    : process.env.AFFECTED_TESTS_BASE || options.base;
  return git(options.repo, [
    "diff",
    "--name-only",
    "--diff-filter=ACDMRTUXB",
    `${base}...HEAD`,
  ]).split(/\r?\n/).filter(Boolean).map((file) => normalizeRepoPath(file, options.repo));
}

function normalizeRepoPath(file, repoRoot) {
  const absolute = path.isAbsolute(file) ? path.resolve(file) : null;
  const relative = absolute ? path.relative(repoRoot, absolute) : file;
  if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`changed file is outside the repository: ${file}`);
  }
  return relative.replaceAll("\\", "/").replace(/^\.\//, "");
}

function runMetadata(repoRoot, manifest) {
  const manifestPath = path.join(repoRoot, manifest);
  if (!existsSync(manifestPath)) return [];
  const json = execFileSync(
    "cargo",
    ["metadata", "--no-deps", "--format-version", "1", "--manifest-path", manifest],
    { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  return JSON.parse(json).packages.map((pkg) => ({
    ...pkg,
    manifestFile: path.resolve(repoRoot, pkg.manifest_path),
    packageRoot: path.dirname(path.resolve(repoRoot, pkg.manifest_path)),
  }));
}

function isInside(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

function packageForPath(repoRoot, file, packages) {
  const absolute = path.resolve(repoRoot, file);
  return packages
    .filter((pkg) => isInside(pkg.packageRoot, absolute))
    .sort((left, right) => right.packageRoot.length - left.packageRoot.length)[0];
}

function packageScope(pkg, file) {
  if (file.startsWith("src-tauri/pty-fork/")) return "pty-fork";
  return scopeAliases.get(pkg.name) || pkg.name;
}

function modulePathForRust(repoRoot, file, pkg) {
  const absolute = path.resolve(repoRoot, file);
  const targetRoots = pkg.targets
    .filter((target) => target.kind.some((kind) => ["lib", "bin", "test"].includes(kind)))
    .map((target) => path.dirname(path.resolve(repoRoot, target.src_path)))
    .filter((sourceRoot) => isInside(sourceRoot, absolute))
    .sort((left, right) => right.length - left.length);
  if (targetRoots.length === 0) return null;

  const relative = path.relative(targetRoots[0], absolute).split(path.sep);
  const last = relative.at(-1) || "";
  if (last === "lib.rs" || last === "main.rs" || last === "build.rs") relative.pop();
  else if (last === "mod.rs") relative.pop();
  else relative[relative.length - 1] = last.replace(/\.rs$/, "");
  const module = relative.filter(Boolean).join("::");
  return module || null;
}

function rustInputs(files) {
  return files.filter((file) =>
    file.endsWith(".rs") ||
    (file.startsWith("src-tauri/") || file.startsWith("cli/")) &&
      ["Cargo.toml", "Cargo.lock"].includes(path.posix.basename(file)),
  );
}

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function addUnique(list, value) {
  if (!list.includes(value)) list.push(value);
}

function getGroup(groups, pkg, scope) {
  if (!groups.has(scope)) {
    groups.set(scope, { package: pkg, scope, filters: new Map(), wholeReasons: [] });
  }
  return groups.get(scope);
}

function addWhole(groups, pkg, scope, reason) {
  const group = getGroup(groups, pkg, scope);
  group.filters.clear();
  addUnique(group.wholeReasons, reason);
}

function addFilter(groups, pkg, scope, filter, reason) {
  const group = getGroup(groups, pkg, scope);
  if (group.wholeReasons.length > 0) {
    addUnique(group.wholeReasons, `${reason}; included by the whole-crate selection`);
    return;
  }
  if (!group.filters.has(filter)) group.filters.set(filter, []);
  addUnique(group.filters.get(filter), reason);
}

function moduleFilter(modulePath) {
  const escaped = modulePath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return `test(/^${escaped}::/)`;
}

function renderRustCommands(groups) {
  const order = ["app", "plan", "pty", "plugin", "types", "cli", "mem", "mesh", "plugins", "pty-fork"];
  return [...groups.values()]
    .sort((left, right) => {
      const leftRank = order.indexOf(left.scope);
      const rightRank = order.indexOf(right.scope);
      return (leftRank < 0 ? order.length : leftRank) - (rightRank < 0 ? order.length : rightRank) || left.scope.localeCompare(right.scope);
    })
    .map((group) => {
      const filters = [...group.filters.keys()];
      const filterset = filters.join(" | ");
      const command = `./build.sh nextest ${group.scope}${filterset ? ` ${shellQuote(filterset)}` : ""}`;
      const reasons = group.wholeReasons.length > 0
        ? group.wholeReasons
        : filters.flatMap((filter) => group.filters.get(filter).map((reason) => `${filter}: ${reason}`));
      const selectionCost = filters.length > 0 ? `${filters.length} filter(s)` : "whole touched crate";
      return {
        command,
        reasonLines: reasons.map((reason) => `  Reason: ${reason}`),
        cost: `  Cost: 1 ${group.package.name} compile + ${selectionCost}`,
      };
    });
}

function selectedTsFiles(files) {
  return files.filter((file) => /\.tsx?$/.test(file));
}

function customUnitFiles(repoRoot, files) {
  const sourceFiles = files.filter((file) =>
    /\.tsx?$/.test(file) && (file.startsWith("src/") || file.startsWith("packages/")),
  );
  if (sourceFiles.length === 0) return [];
  const tests = discoverTests({}, repoRoot);
  return selectRelatedTests(tests, sourceFiles, repoRoot);
}

function vitestFiles(repoRoot, files) {
  const listPath = path.join(repoRoot, "scripts/vitest-owned-tests.json");
  if (!existsSync(listPath)) return [];
  const tests = JSON.parse(readFileSync(listPath, "utf8"));
  const relatedRoots = selectedTsFiles(files).some((file) =>
    file.startsWith("src/") || file.startsWith("packages/") || file.startsWith("plugins/analytics/"),
  );
  return relatedRoots ? tests : [];
}

function pluginCommands(files) {
  const directories = [...new Set(files
    .filter((file) => file.startsWith("plugins/"))
    .map((file) => file.split("/")[1])
    .filter(Boolean))].sort();
  const commands = [];
  const providerSuites = ["provider-claude", "provider-codex", "provider-opencode", "provider-cursor"];
  for (const provider of directories) {
    if (provider === "analytics") continue;
    const suites = provider === "provider-shared" ? providerSuites : [provider];
    for (const suite of suites) {
      commands.push({
        command: `bash scripts/plugin-tests.sh ${suite}`,
        reason: provider === "provider-shared"
          ? `shared provider code can affect ${suite} plugin tests`
          : `changed plugin files belong to ${provider}`,
      });
    }
  }
  return commands;
}

function renderParallelCommands(repoRoot, files) {
  const commands = [];
  const unitTests = customUnitFiles(repoRoot, files);
  const unitFiles = files.filter((file) =>
    /\.tsx?$/.test(file) && (file.startsWith("src/") || file.startsWith("packages/")),
  );
  if (unitTests.length > 0) {
    commands.push({
      command: `pnpm test:unit -- --related ${unitFiles.map(shellQuote).join(" ")}`,
      reason: `changed TS files map to ${unitTests.length} related tsx-runner test file(s)`,
    });
  }

  const relatedVitest = vitestFiles(repoRoot, files);
  if (relatedVitest.length > 0) {
    commands.push({
      command: `pnpm exec vitest related --run --maxWorkers=2 --passWithNoTests ${selectedTsFiles(files).map(shellQuote).join(" ")}`,
      reason: "Vitest resolves related test files from the changed source graph",
    });
  }
  commands.push(...pluginCommands(files));
  if (files.some((file) => file.startsWith("essences/"))) {
    commands.push({
      command: "codeterm essence-map validate --root .",
      reason: "essence sources changed",
    });
  }
  return commands;
}

function loadPackageSets(options, files) {
  const sets = [];
  if (files.some((file) => file.startsWith("src-tauri/") || file.startsWith("plugins/provider-"))) {
    sets.push(...runMetadata(options.repo, "src-tauri/Cargo.toml"));
  }
  if (files.some((file) => file.startsWith("cli/")) || files.some((file) => file.startsWith("prod-docs/"))) {
    sets.push(...runMetadata(options.repo, "cli/Cargo.toml"));
  }
  if (files.some((file) => file.startsWith("src-tauri/pty-fork/"))) {
    sets.push(...runMetadata(options.repo, "src-tauri/pty-fork/Cargo.toml"));
  }
  return sets;
}

function affectedPackages(repoRoot, inputs, packages) {
  const result = new Map();
  const add = (pkg, file) => {
    if (!pkg) return;
    const key = pkg.manifestFile;
    if (!result.has(key)) result.set(key, { package: pkg, files: [] });
    addUnique(result.get(key).files, file);
  };

  const workspaceWide = inputs.some((file) => file === "src-tauri/Cargo.toml" || file === "src-tauri/Cargo.lock");
  if (workspaceWide) {
    for (const pkg of packages.filter((item) => item.manifestFile.startsWith(path.join(repoRoot, "src-tauri") + path.sep))) {
      add(pkg, "src-tauri/Cargo.toml or Cargo.lock changed");
    }
  }

  for (const file of inputs) {
    if (file === "src-tauri/Cargo.toml" || file === "src-tauri/Cargo.lock") continue;
    const directManifest = packages.find((pkg) =>
      path.resolve(repoRoot, file) === pkg.manifestFile,
    );
    if (directManifest) {
      add(directManifest, file);
      continue;
    }
    const pkg = packageForPath(repoRoot, file, packages);
    add(pkg, file);
  }
  return [...result.values()];
}

function printSelection(options, files) {
  const changedRust = rustInputs(files);
  if (changedRust.length > 0 && !options.rustMode) {
    throw new Error("Rust files changed; pass --rust-mode whole-crate or --rust-mode module-filters");
  }

  const packages = loadPackageSets(options, files);
  const groups = new Map();
  const affected = affectedPackages(options.repo, changedRust, packages);
  for (const entry of affected) {
    const { package: pkg, files: packageFiles } = entry;
    const scope = packageScope(pkg, packageFiles[0]);
    if (options.rustMode === "whole-crate") {
      addWhole(groups, pkg, scope, `whole touched crate selected for ${packageFiles.join(", ")}`);
      continue;
    }
    if (pkg.name !== "codeterm") {
      addFilter(
        groups,
        pkg,
        scope,
        `rdeps(${pkg.name})`,
        `${packageFiles.join(", ")} changed; select tests in reverse-dependent packages`,
      );
      continue;
    }
    for (const file of packageFiles.filter((candidate) => candidate.endsWith(".rs"))) {
      const modulePath = modulePathForRust(options.repo, file, pkg);
      if (modulePath) {
        const filter = moduleFilter(modulePath);
        addFilter(groups, pkg, scope, filter, `${file} maps to Rust module ${modulePath}`);
      } else {
        addWhole(groups, pkg, scope, `${file} has no module path; run the touched app crate`);
      }
    }
    if (packageFiles.some((file) => !file.endsWith(".rs"))) {
      addWhole(groups, pkg, scope, `${packageFiles.filter((file) => !file.endsWith(".rs")).join(", ")} changes app package configuration`);
    }
  }

  const packageByName = new Map(packages.map((pkg) => [pkg.name, pkg]));
  const contract = (packageName, filter, reason) => {
    const pkg = packageByName.get(packageName);
    if (!pkg) throw new Error(`cargo metadata did not contain ${packageName}`);
    addFilter(groups, pkg, packageScope(pkg, ""), filter, reason);
  };

  for (const file of files.filter((candidate) => candidate.startsWith("src-tauri/prompts/"))) {
    contract("codeterm", "test(/^prompts::tests::/)", `${file} changes the prompt contract suite`);
  }
  for (const file of files.filter((candidate) => candidate.startsWith("prod-docs/"))) {
    contract(
      "codeterm-cli",
      "test(/registered_documents_have_one_title_and_fit_the_reading_boundary/)",
      `${file} changes the embedded production-doc boundary`,
    );
  }

  const providerChanged = files.some((file) =>
    file.startsWith("plugins/provider-") || file.startsWith("src-tauri/crates/codeterm-plugin/"),
  );
  if (providerChanged) {
    contract("codeterm-plugin", "test(/parity_test/)", "provider/plugin code changed; run native parity coverage");
  }

  const rustCommands = renderRustCommands(groups);
  const parallelCommands = renderParallelCommands(options.repo, files);
  const rustChanged = changedRust.length > 0;
  const compileFallback = rustChanged && rustCommands.length === 0;

  if (compileFallback) {
    console.log("Rust compile fallback");
    console.log("./build.sh check-tests");
    console.log("  Reason: Rust changes were found but cargo metadata selected no test crate.");
    console.log("  Cost: app and CLI test-profile compile; no test filters selected.");
  }
  if (rustCommands.length > 0) {
    console.log("Rust");
    for (const selection of rustCommands) {
      for (const reason of selection.reasonLines) console.log(reason);
      console.log(selection.command);
      console.log(selection.cost);
    }
  }
  if (parallelCommands.length > 0) {
    console.log("Parallel while Rust compiles");
    for (const selection of parallelCommands) {
      console.log(`  Reason: ${selection.reason}`);
      console.log(selection.command);
      console.log("  Cost: no Cargo build gateway; parallelizable with Rust compilation.");
    }
  }
  if (rustCommands.length === 0 && parallelCommands.length === 0 && !compileFallback) {
    console.log("No affected test commands selected.");
  }
}

function printHelp() {
  console.log("Usage: node scripts/affected-tests.mjs [--base REF] [--rust-mode whole-crate|module-filters] [changed-file ...]");
  console.log("Without file arguments, reads git diff --name-only <base>...HEAD (default base: origin/master).");
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) return printHelp();
  const files = [...new Set(changedFiles(options))].sort();
  if (files.length === 0) {
    console.log("No changed files.");
    return;
  }
  console.log(`Affected tests for ${files.length} changed file(s):`);
  printSelection(options, files);
}

try {
  main();
} catch (error) {
  console.error(`affected-tests: ${error.message}`);
  process.exitCode = 2;
}
