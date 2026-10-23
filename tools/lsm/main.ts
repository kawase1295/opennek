import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

// True when the module was started directly (`tsx tools/x.ts`), not imported by a test
export const isMain = (url: string) => !!process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === url
