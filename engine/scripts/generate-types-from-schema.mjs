import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compileFromFile } from "json-schema-to-typescript";

const here = dirname(fileURLToPath(import.meta.url));
const engineRoot = resolve(here, "..");
const schemasDir = resolve(engineRoot, "schemas");
const outFile = resolve(
  engineRoot,
  "crates",
  "web_binding",
  "types",
  "generated",
  "contract.generated.d.ts",
);

const requestSchema = resolve(schemasDir, "ipa_stream.request.schema.json");
const responseSchema = resolve(
  schemasDir,
  "stream_analysis.response.schema.json",
);

async function run() {
  const requestTs = await compileFromFile(requestSchema, {
    bannerComment: "",
    unknownAny: false,
    strictIndexSignatures: true,
  });

  const responseTs = await compileFromFile(responseSchema, {
    bannerComment: "",
    unknownAny: false,
    strictIndexSignatures: true,
  });

  const output = [
    "/* eslint-disable */",
    "// AUTO-GENERATED FILE. DO NOT EDIT MANUALLY.",
    "// Source: engine/schemas/*.schema.json (generated via schemars from Rust).",
    "",
    requestTs.trim(),
    "",
    responseTs.trim(),
    "",
  ].join("\n");

  await mkdir(dirname(outFile), { recursive: true });
  await writeFile(outFile, output, "utf8");
  console.log(`Wrote ${outFile}`);
}

run().catch((error) => {
  console.error(error);
  process.exit(1);
});
