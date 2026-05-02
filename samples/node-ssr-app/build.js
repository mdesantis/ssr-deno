const esbuild = require('esbuild')
const config = require('./esbuild.config')

esbuild.build(config).then(() => {
  const fs = require('fs')
  const size = fs.statSync(config.outfile).size
  console.log(`Bundle built successfully: ${(size / 1024).toFixed(1)} KB`)
}).catch((err) => {
  console.error('Build failed:', err)
  process.exit(1)
})
