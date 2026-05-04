// Streaming bundle that pushes HTML chunks via __ssr_push_chunk.
// Used by test_deno_render_stream_chunks.rb to verify chunked delivery.
globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  var name = (parsed.data && parsed.data.name) || 'world';

  return new Promise(function(resolve, reject) {
    try {
      globalThis.__ssr_push_chunk('<html><body>');
      globalThis.__ssr_push_chunk('<h1>' + name + '</h1>');
      globalThis.__ssr_push_chunk('</body></html>');
      resolve('done');
    } catch (e) {
      reject(e);
    }
  });
};
