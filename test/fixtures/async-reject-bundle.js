globalThis.render = function(data) {
  return Promise.reject(new Error('async-rejection'));
};
