globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  return '<div>' + JSON.stringify(parsed) + '</div>';
};
