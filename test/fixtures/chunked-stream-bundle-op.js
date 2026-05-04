// Streaming bundle that pushes HTML chunks via the async op-based path.
// Uses `await globalThis.__ssr_push_chunk_op(string)` which flows through
// the Deno op system with true backpressure.
// Used by test_deno_render_stream_chunks_op.rb to verify op-based delivery.
globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  var name = (parsed.data && parsed.data.name) || 'world';

  return new Promise(async function(resolve, reject) {
    try {
      await globalThis.__ssr_push_chunk_op('<html><body>');
      await globalThis.__ssr_push_chunk_op('<h1>' + name + '</h1>');
      await globalThis.__ssr_push_chunk_op('</body></html>');
      resolve('done');
    } catch (e) {
      reject(e);
    }
  });
};
