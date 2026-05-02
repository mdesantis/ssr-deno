globalThis.render = function (argsJson) {
  var data = JSON.parse(argsJson)
  var name = data.name || 'World'
  return '<!DOCTYPE html>\n<html>\n  <head><title>Hello ' + name + '</title></head>\n  <body>\n    <div id="root">\n      <h1>Hello ' + name + '!</h1>\n      <p>Plain JS SSR bundle — no framework, no build step, no Deno APIs.</p>\n    </div>\n  </body>\n</html>'
}
