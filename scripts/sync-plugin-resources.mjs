import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = join(__dirname, "..");
const profile = process.argv[2] === "release" ? "release" : "debug";
const exeSuffix = process.platform === "win32" ? ".exe" : "";

const plugins = ["niuma-codex-plugin", "niuma-plugin-bark", "niuma-plugin-ntfy"];
const outputDir = join(rootDir, "src-tauri", "resources", "bin");

mkdirSync(outputDir, { recursive: true });

for (const plugin of plugins) {
  const executableName = `${plugin}${exeSuffix}`;
  const source = join(rootDir, "target", profile, executableName);
  const destination = join(outputDir, executableName);

  if (!existsSync(source)) {
    throw new Error(`插件二进制不存在，请先构建：${source}`);
  }

  // Tauri 打包只读取 resources 目录，插件构建后需要同步到这个稳定位置。
  copyFileSync(source, destination);
  console.log(`synced ${plugin} -> ${destination}`);
}
