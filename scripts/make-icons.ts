/**
 * Renders assets/icon.svg to assets/icon.png (1024 px), the master raster the
 * bundled icons in src-tauri/icons/ are made from (`bunx tauri icon`, keeping
 * only the files tauri.conf.json lists). Uses macOS `sips`, so it runs on a Mac.
 * Re-run after editing icon.svg:  bun run icons
 */
import { $ } from "bun";
import { existsSync, statSync } from "fs";

if (process.platform !== "darwin")
  throw new Error("make-icons needs macOS (sips). Icons are committed; regenerate on a Mac.");

const SVG = "assets/icon.svg";
const OUT = "assets/icon.png";

await $`sips -s format png -Z 1024 ${SVG} --out ${OUT}`.quiet();
if (!existsSync(OUT) || statSync(OUT).size === 0)
  throw new Error(`make-icons: sips produced no output for ${SVG}`);
console.log(`icons: ${OUT}`);
