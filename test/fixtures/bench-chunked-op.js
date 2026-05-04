// Benchmark bundle: pushes N chunks via the OP-BASED path.
// N is controlled by the `count` field in the input data.
globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  var count = (parsed && parsed.count) || 10;

  return new Promise(async function(resolve, reject) {
    try {
      for (var i = 0; i < count; i++) {
        await globalThis.__ssr_push_chunk_op('<div>chunk-' + i + '</div>');
      }
      resolve('done');
    } catch (e) {
      reject(e);
    }
  });
};
