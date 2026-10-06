const fs = require("fs"),
  http = require("http"),
  path = require("path");
const {
  chromium,
} = require("/home/panshuainan/.nvm/versions/node/v24.18.0/lib/node_modules/openclaw/node_modules/playwright-core");
const output = process.env.R7_EVIDENCE_DIR;
if (!output) throw Error("R7_EVIDENCE_DIR required");
(async () => {
  const root = __dirname;
  const server = http.createServer((req, res) => {
    const p = path.join(root, req.url === "/" ? "index.html" : req.url);
    res.setHeader(
      "Content-Type",
      p.endsWith(".js") ? "application/javascript" : "text/html"
    );
    const stream = fs.createReadStream(p);
    stream.on("error", () => {
      res.statusCode = 404;
      res.end();
    });
    stream.pipe(res);
  });
  await new Promise((r) => server.listen(18811, "127.0.0.1", r));
  let browser;
  try {
    browser = await chromium.launch({
      executablePath: "/usr/bin/google-chrome-stable",
      headless: true,
      args: ["--no-sandbox"],
    });
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.route("http://127.0.0.1:18810/**", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: '{"mtime":123}',
      })
    );
    await page.goto("http://127.0.0.1:18811");
    await page.locator("#editor").waitFor();
    const startup = await page.evaluate(() => window.r7);
    if (
      startup.path !== "/fixture/small.txt" ||
      startup.counters.largeRead !== 0
    )
      throw Error("Unsafe restoration");
    await page.locator("#large").click();
    await page.locator("#blocked").waitFor();
    if (await page.locator("#editor").count())
      throw Error("Blocked editor model exists");
    if (!(await page.locator("#save").isDisabled()))
      throw Error("Blocked save enabled");
    const blocked = await page.evaluate(() => window.r7);
    if (blocked.counters.largeRead !== 0 || blocked.counters.write !== 0)
      throw Error("Blocked IO");
    await page.screenshot({
      path: path.join(output, "r7-controlled-ui-blocked.png"),
    });
    await page.locator("#small").click();
    await page.locator("#editor").waitFor();
    await page.locator("#editor").fill("edited 小雨");
    await page.locator("#save").click();
    await page.locator("#saved").waitFor();
    await page.reload();
    await page.locator("#editor").waitFor();
    if ((await page.locator("#editor").inputValue()) !== "edited 小雨")
      throw Error("Small edit-save-reload regression");
    const reload = await page.evaluate(() => window.r7);
    if (errors.length) throw Error(errors.join("\n"));
    fs.writeFileSync(
      path.join(output, "r7-controlled-ui.json"),
      JSON.stringify(
        {
          kind: "Actual hook + hydrate + queue, mock FS, isolated Chromium localStorage; not full native app",
          startup,
          blocked,
          reload,
          errors,
        },
        null,
        2
      )
    );
    console.log("R7 controlled UI PASS");
  } finally {
    if (browser) await browser.close();
    server.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
