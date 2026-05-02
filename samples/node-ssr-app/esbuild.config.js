const path = require('path')

module.exports = {
  entryPoints: [path.resolve(__dirname, 'src/entry-server.ts')],
  outfile: path.resolve(__dirname, 'dist/server/entry-server.js'),
  bundle: true,
  format: 'iife',
  target: 'es2022',
  platform: 'browser',
  minify: false,
  sourcemap: false,
}
