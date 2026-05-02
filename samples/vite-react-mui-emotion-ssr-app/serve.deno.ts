/// <reference types="@types/deno" />
import { createRequire } from 'node:module'

const PORT = parseInt(Deno.env.get("PORT") || "3109", 10);
const BUNDLE_PATH = new URL("./dist/server/entry-server.js", import.meta.url);

const bundleCode = await Deno.readTextFile(BUNDLE_PATH);
const scriptCode = bundleCode.replace(/export\s+\{[^}]+\};?\s*$/, "");

const require = createRequire(BUNDLE_PATH);

const fn = new Function('require', `
  ${scriptCode}
  return typeof render !== "undefined" ? render : null;
`);

const renderFn = fn(require);
if (typeof renderFn !== "function") {
  console.error("Bundle did not export a render function");
  Deno.exit(1);
}

const sizeKB = (bundleCode.length / 1024).toFixed(0);
console.log(`Bundle loaded successfully (${sizeKB} KB)`);

Deno.serve({ port: PORT }, (req: Request) => {
  const url = new URL(req.url);
  const name = url.searchParams.get("name") || "World";

  try {
    const result = renderFn(JSON.stringify({ data: { name } }));
    const { html, css } = JSON.parse(result);
    const fullHtml = `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8">${css}<title>Hello ${name}</title></head>
  <body><div id="root">${html}</div></body>
</html>`;
    return new Response(fullHtml, {
      status: 200,
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("Render error:", message);
    return new Response(`Render error: ${message}`, { status: 500 });
  }
});

console.log(`React MUI Emotion SSR test server running at http://localhost:${PORT}`);
console.log(`Try: http://localhost:${PORT}?name=Maurizio`);
