// Runs during extension initialization (before bootstrap finalizes).
// Captures op_ssr_push_chunk from core.ops and exposes it as an async global.
// After bootstrap, Deno.core is hidden - this is the only window to grab the op.
//
// This is a classic script (js_files, not esm_files) - no import statements.
// Deno.core is available as a global during extension initialization.
const __op_push_chunk = Deno.core.ops.op_ssr_push_chunk;

globalThis.__ssr_push_chunk_op = function(chunk) {
  return __op_push_chunk(chunk);
};
