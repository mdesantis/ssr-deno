globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  var name = (parsed.data && parsed.data.name) || 'world';
  return '<h1>' + name + '</h1>';
};
