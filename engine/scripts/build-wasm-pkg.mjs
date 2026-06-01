import {
  existsSync,
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const root = resolve(scriptDir, "..");
const pkgDir = resolve(root, "crates/web_binding/pkg-bundler");
const pkgJsonPath = resolve(pkgDir, "package.json");
const wasmGluePath = resolve(pkgDir, "ipa_poetry_engine_bg.js");

function readPreviousVersion(path) {
  if (!existsSync(path)) return null;
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    return typeof parsed.version === "string" ? parsed.version : null;
  } catch {
    return null;
  }
}

const previousPkgVersion = readPreviousVersion(pkgJsonPath);

// ── locate wasm-pack ───────────────────────────────────────────────────────
const wasmPackCandidates = [
  "wasm-pack",
  resolve(homedir(), ".cargo", "bin", "wasm-pack.exe"),
  resolve(homedir(), ".cargo", "bin", "wasm-pack"),
];

const wasmPack = wasmPackCandidates.find((c) => {
  if (c === "wasm-pack") {
    return spawnSync(c, ["--version"], { stdio: "ignore" }).status === 0;
  }
  return existsSync(c);
});

if (!wasmPack) {
  console.error("Cannot find wasm-pack. Install it: cargo install wasm-pack");
  process.exit(1);
}

const cargoBin = resolve(homedir(), ".cargo", "bin");
const pathSep = process.platform === "win32" ? ";" : ":";
const childPath = `${cargoBin}${pathSep}${process.env.PATH ?? ""}`;

// ── wasm-pack build ────────────────────────────────────────────────────────
if (existsSync(pkgDir)) {
  console.log("==> Removing stale pkg-bundler/");
  rmSync(pkgDir, { recursive: true, force: true });
}

console.log("==> wasm-pack build --target bundler");
const result = spawnSync(
  wasmPack,
  [
    "build",
    "crates/web_binding",
    "--target",
    "bundler",
    "--release",
    "--out-dir",
    "pkg-bundler",
  ],
  {
    stdio: "inherit",
    cwd: root,
    env: { ...process.env, PATH: childPath },
  },
);

if (result.status !== 0) process.exit(result.status ?? 1);

// ── patch wasm-bindgen glue for Vite/Firefox module duplication edge-case ─
// In some Vite worker graphs Firefox can evaluate two instances of
// ipa_poetry_engine_bg.js with different query strings. One instance receives
// __wbg_set_wasm(), while the other services imported callbacks from WASM.
// Share the wasm instance through globalThis to keep externref init stable.
console.log("==> Patching wasm glue for duplicate-module safety");
const wasmGlue = readFileSync(wasmGluePath, "utf8");
const patchedWasmGlue = wasmGlue
  .replace(
    "export function __wbindgen_init_externref_table() {\n    const table = wasm.__wbindgen_externrefs;",
    "export function __wbindgen_init_externref_table() {\n    const wasmInstance = wasm ?? globalThis.__ipa_poetry_engine_wasm;\n    if (!wasmInstance) {\n        throw new Error('ipa-poetry-engine: WASM module is not initialised');\n    }\n    const table = wasmInstance.__wbindgen_externrefs;",
  )
  .replace(
    "export function __wbg_set_wasm(val) {\n    wasm = val;\n}",
    "export function __wbg_set_wasm(val) {\n    wasm = val;\n    globalThis.__ipa_poetry_engine_wasm = val;\n}",
  );

if (patchedWasmGlue !== wasmGlue) {
  writeFileSync(wasmGluePath, patchedWasmGlue, "utf8");
} else {
  console.warn("[warn] wasm glue patch was not applied (template mismatch)");
}

// ── patch package.json ─────────────────────────────────────────────────────
console.log("==> Patching pkg-bundler/package.json");
const pkgJson = JSON.parse(readFileSync(pkgJsonPath, "utf8"));

pkgJson.name = "ipa-poetry-engine";
pkgJson.version = previousPkgVersion ?? pkgJson.version;
pkgJson.description = "IPA poetry analysis engine - WebAssembly / npm binding";
pkgJson.license = "AGPL-3.0-or-later";
pkgJson.author = "Tilitronic";
pkgJson.repository = {
  type: "git",
  url: "https://github.com/Tilitronic/ipa-poetry-engine.git",
  directory: "engine/crates/web_binding",
};
pkgJson.main = "ipa_poetry_engine.js";
pkgJson.module = "ipa_poetry_engine.js";
pkgJson.types = "types.d.ts";
pkgJson.exports = {
  ".": {
    types: "./types.d.ts",
    import: "./ipa_poetry_engine.js",
    default: "./ipa_poetry_engine.js",
  },
  "./ipa_poetry_engine_bg.wasm": "./ipa_poetry_engine_bg.wasm",
};
pkgJson.files = [
  "ipa_poetry_engine_bg.wasm",
  "ipa_poetry_engine_bg.js",
  "ipa_poetry_engine.js",
  "ipa_poetry_engine.d.ts",
  "ipa_poetry_engine_bg.wasm.d.ts",
  "types.d.ts",
  "LICENSE",
];
pkgJson.sideEffects = ["./ipa_poetry_engine.js", "./snippets/*"];
pkgJson.keywords = [
  "poetry",
  "phonetics",
  "ipa",
  "wasm",
  "webassembly",
  "nlp",
  "vite",
  "bundler",
];
pkgJson.engines = { node: ">=18.0.0" };

writeFileSync(pkgJsonPath, `${JSON.stringify(pkgJson, null, 2)}\n`, "utf8");

// ── copy extras ────────────────────────────────────────────────────────────
console.log("==> Copying types.d.ts");
copyFileSync(
  resolve(root, "crates/web_binding/types/index.d.ts"),
  resolve(pkgDir, "types.d.ts"),
);

console.log("==> Copying LICENSE");
copyFileSync(resolve(root, "../LICENSE"), resolve(pkgDir, "LICENSE"));

// ── summary ───────────────────────────────────────────────────────────────
console.log("\nDone. pkg-bundler/ ready at:");
console.log(`  ${pkgDir}`);
