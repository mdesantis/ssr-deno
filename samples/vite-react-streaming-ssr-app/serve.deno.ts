/// <reference types="@types/deno" />

/**
 * Deno test server for the React Streaming SSR bundle.
 *
 * This simulates what the Ruby gem will do via deno_runtime::MainWorker:
 * 1. Load the built SSR bundle (self-contained, all deps inlined)
 * 2. Evaluate it in a V8 context
 * 3. Call the render() function with JSON data (async, collects stream chunks)
 * 4. Serve the resulting HTML via HTTP
 *
 * Usage:
 *   deno task serve
 *
 * Then open http://localhost:3114?name=Developer
 */

const PORT = parseInt(Deno.env.get("PORT") || "3114", 10);
const BUNDLE_PATH = new URL("./dist/server/entry-server.js", import.meta.url);

// Set up require() for Node.js built-in modules (util, crypto, stream, async_hooks)
// that are externalized by the Vite SSR build but needed by react-dom/server.node.
import { createRequire } from "node:module";
const nodeRequire = createRequire(import.meta.url);
if (typeof globalThis.require === "undefined") {
  globalThis.require = nodeRequire;
}

const bundleCode = await Deno.readTextFile(BUNDLE_PATH);

const scriptCode = bundleCode.replace(/export\s+\{[^}]+\};?\s*$/, "");

const fn = new Function("require", `
  ${scriptCode}
  return typeof render !== "undefined" ? render : null;
`);

let renderFn: ((argsJson: string) => unknown) | null;

try {
  renderFn = fn(nodeRequire) as typeof renderFn;
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

Deno.serve({ port: PORT }, async (req: Request) => {
  const url = new URL(req.url);
  const name = url.searchParams.get("name") || "World";

  console.log(`[${new Date().toISOString()}] ${req.method} ${url.pathname}${url.search}`);

  try {
    const result = renderFn(JSON.stringify({ data: { name } }));

    const html = result instanceof Promise ? await result : String(result);

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

console.log(`React Streaming SSR test server running at http://localhost:${PORT}`);
console.log(`Try: http://localhost:${PORT}?name=Developer`);
