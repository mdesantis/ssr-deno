globalThis.render = function (data: string) {
  var parsed = JSON.parse(data);
  if (typeof __ssr_push_chunk !== 'undefined') {
    __ssr_push_chunk('<div>hello</div>');
  }
  return { hello: 'world', input: parsed.input };
};
