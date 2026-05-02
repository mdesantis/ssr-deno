/// <reference types="@types/deno" />

const PORT = parseInt(Deno.env.get("PORT") || "3112", 10);
const BUNDLE_PATH = new URL("./dist/server/entry-server.js", import.meta.url);

const bundleCode = await Deno.readTextFile(BUNDLE_PATH);

const fn = new Function(`
  ${bundleCode}
  return typeof globalThis.render !== "undefined" ? globalThis.render : null;
`);

const renderFn = fn();
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
    const html = renderFn(JSON.stringify({ data: { name } }));
    return new Response(html, {
      status: 200,
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("Render error:", message);
    return new Response(`Render error: ${message}`, { status: 500 });
  }
});

console.log(`Webpack React SSR test server running at http://localhost:${PORT}`);
console.log(`Try: http://localhost:${PORT}?name=Maurizio`);
