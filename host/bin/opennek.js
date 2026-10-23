#!/usr/bin/env node
// node:sqlite prints an ExperimentalWarning on every start; drop that one before the host loads it
const [major, minor] = process.versions.node.split('.').map(Number)
if (major < 22 || (major === 22 && minor < 13)) {
  console.error(`OpenNEK needs Node.js 22.13 or later (this is ${process.version}).`)
  process.exit(1)
}
process.removeAllListeners('warning')
process.on('warning', (w) => {
  if (w.name !== 'ExperimentalWarning') console.warn(w.stack ?? String(w))
})
await import('../dist/index.js')
