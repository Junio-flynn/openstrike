import { mkdir } from "node:fs/promises";
import { compilePocketTarget, pocketOutputDirectory } from "./pocket-contract.ts";

const argv = Bun.argv.slice(2);
const targetIndex = argv.indexOf("--target");
const target = targetIndex >= 0 ? argv[targetIndex + 1] : undefined;
if (!target || argv.length !== 2 || targetIndex !== 0) {
  console.error("usage: bun scripts/build-ui.ts --target <target>");
  process.exit(1);
}

if (target === "desktop") {
  // Desktop intentionally uses the PSP-baseline capabilities and raster
  // assets, but its native host contract must still identify as desktop.
  const plan = await compilePocketTarget("psp");
  const source = pocketOutputDirectory("psp");
  const output = pocketOutputDirectory("desktop");
  const js = (await Bun.file(`${source}/${plan.appOutput}.js`).text())
    .replace('const target = "psp";', 'const target = "desktop";')
    // The current desktop UI surface predates __hostAbi; keep its existing
    // legacy contract behavior until that native surface publishes an ABI.
    .replace("const hostAbi = 1;", "const hostAbi = 0;")
    .replace('target: "psp",', 'target: "desktop",');
  await mkdir(output, { recursive: true });
  await Bun.write(`${output}/${plan.appOutput}.js`, js);
  await Bun.write(`${output}/${plan.appOutput}.pak`, Bun.file(`${source}/${plan.appOutput}.pak`));
  console.log(`output: ${output}/${plan.appOutput}.{js,pak}`);
} else {
  const plan = await compilePocketTarget(target);
  console.log(`output: ${pocketOutputDirectory(target)}/${plan.appOutput}.{js,pak}`);
}
