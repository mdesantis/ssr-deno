/// <reference types="@types/deno" />

/**
 * Deno test server for the React SSR bundle.
 *
 * This simulates what the Ruby gem will do via deno_runtime::MainWorker:
 * 1. Load the built SSR bundle (self-contained, all deps inlined)
 * 2. Evaluate it in a V8 context
 * 3. Call the render() function with JSON data
 * 4. Serve the resulting HTML via HTTP
 *
 * The bundle uses __commonJSMin wrappers for inlined CJS deps (react, react-dom)
 * and has `export { render }` at the end. Since MainWorker evaluates JS
 * directly via V8's execute_script (not through Deno's module loader), we strip
 * the ESM export and evaluate as a plain script — exactly what the Rust extension
 * will do.
 *
 * Usage:
 *   deno run --allow-read --allow-net serve.deno.ts
 *
 * Then open http://localhost:3107?name=Developer
 */

const PORT = parseInt(Deno.env.get("PORT") || "3107", 10);
const BUNDLE_PATH = new URL("./dist/server/entry-server.js", import.meta.url);

// Load the built SSR bundle
const bundleCode = await Deno.readTextFile(BUNDLE_PATH);

// Strip the ESM export line — the bundle uses __commonJSMin wrappers for
// inlined CJS deps, so it can't be imported as ESM. We evaluate it as a
// plain script, which is exactly what MainWorker::execute_script will do.
const scriptCode = bundleCode.replace(/export\s+\{[^}]+\};?\s*$/, "");

const fn = new Function(`
  ${scriptCode}
  return typeof render !== "undefined" ? render : null;
`);

let renderFn: ((url: string, context: Record<string, unknown>) => string) | null;

try {
  renderFn = fn() as typeof renderFn;
} catch (err) {
  console.error("Failed to evaluate bundle:", err);
  Deno.exit(1);
}

if (typeof renderFn !== "function") {
  console.error("Bundle did not export a render function");
  Deno.exit(1);
}

const sizeKB = (bundleCode.length / 1024).toFixed(0);
console.log(`Bundle loaded successfully (${sizeKB} KB)`);

// Start HTTP server
Deno.serve({ port: PORT }, (req: Request) => {
  const url = new URL(req.url);
  const name = url.searchParams.get("name") || "World";

  console.log(`[${new Date().toISOString()}] ${req.method} ${url.pathname}${url.search}`);

  try {
    const html = renderFn(url.pathname, {
      component_data: { name },
      props: { extraData: { timestamp: Date.now(), source: "ssr-deno-sample" } },
    });

    return new Response(html, {
      status: 200,
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("Render error:", message);
    return new Response(`Render error: ${message}`, {
      status: 500,
      headers: { "Content-Type": "text/plain" },
    });
  }
});

console.log(`React SSR test server running at http://localhost:${PORT}`);
console.log(`Try: http://localhost:${PORT}?name=Developer`);
