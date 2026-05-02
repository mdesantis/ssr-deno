globalThis.render = function(data) {
  return Promise.resolve().then(function() {
    return '<h1>async-chained</h1>';
  });
};
