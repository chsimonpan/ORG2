const path = require("path");
const root = path.resolve(__dirname, "../..");
const esbuild = require(
  path.join(root, "node_modules/.pnpm/esbuild@0.27.4/node_modules/esbuild")
);
esbuild.buildSync({
  entryPoints: [path.join(__dirname, "entry.tsx")],
  outfile: path.join(__dirname, "bundle.js"),
  bundle: true,
  platform: "browser",
  format: "iife",
  jsx: "automatic",
  alias: {
    "@src": path.join(root, "src"),
    "@tauri-apps/plugin-fs": path.join(__dirname, "fs-fixture.ts"),
    "@src/hooks/logger": path.join(__dirname, "logger-fixture.ts"),
  },
  define: {
    "process.env.NODE_ENV": '"production"',
    "process.env.ORGII_IDE_SERVER_PORT": '"18810"',
    "process.env.ORGII_E2E": '"0"',
  },
});
require("fs").writeFileSync(
  path.join(__dirname, "index.html"),
  '<!doctype html><html><meta charset="utf-8"><title>R7 isolated fixture</title><div id="root"></div><script src="bundle.js"></script></html>'
);
