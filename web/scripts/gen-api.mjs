// 从后端拉取 OpenAPI 并生成 TS 类型:后端先启动(cargo run -p codeplan-server)
// 用法:pnpm gen:api [baseUrl](默认 http://localhost:8080)
import { execSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const baseUrl = process.argv[2] ?? "http://localhost:8080";

const res = await fetch(`${baseUrl}/api/docs/openapi.json`);
if (!res.ok) {
  console.error(`gen:api 拉取 OpenAPI 失败: ${res.status}(后端是否已启动?)`);
  process.exit(1);
}
const spec = await res.json();

const specPath = path.join(webRoot, "packages/api-client/openapi.json");
writeFileSync(specPath, JSON.stringify(spec, null, 2) + "\n");
console.log(`  OpenAPI 已保存 (${Object.keys(spec.paths ?? {}).length} 个路径)`);

execSync(
  "pnpm exec openapi-typescript packages/api-client/openapi.json -o packages/api-client/src/generated.ts --root-info-tslint-ignore --enum",
  { cwd: webRoot, stdio: "inherit" },
);
console.log("gen:api 完成 ✓");
