import { readFile, writeFile } from "node:fs/promises";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("用法：npm run release:version -- 0.2.0");
  process.exit(1);
}

async function updateJson(path, mutate) {
  const value = JSON.parse(await readFile(path, "utf8"));
  mutate(value);
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
}

await updateJson("package.json", (value) => {
  value.version = version;
});

await updateJson("package-lock.json", (value) => {
  value.version = version;
  if (value.packages?.[""]) value.packages[""].version = version;
});

await updateJson("src-tauri/tauri.conf.json", (value) => {
  value.version = version;
});

const cargoPath = "src-tauri/Cargo.toml";
const cargo = await readFile(cargoPath, "utf8");
const updatedCargo = cargo.replace(
  /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+("\s*\n)/,
  `$1${version}$2`,
);
if (updatedCargo === cargo) {
  console.error("未能更新 src-tauri/Cargo.toml 中的 package.version");
  process.exit(1);
}
await writeFile(cargoPath, updatedCargo);

console.log(`Kotoba Atelier 版本已统一更新为 ${version}`);
