import { constants, createReadStream, existsSync, openSync, readdirSync, realpathSync, writeSync } from 'node:fs'

// Minimal Linux serial port for the CLI tools: stty for line settings, plain fd for I/O.
// Through the OpenNEK ESP32-S3 bridge (hardware USB serial, which ignores the host's baud rate) the
// UART baud rate is changed with the bridge's in-band command instead.
export type Port = {
  write(data: string | Uint8Array): void
  setBaud(baud: number): Promise<void>
  readByte(timeoutMs: number): Promise<number | null>
  readUntil(text: string, timeoutMs: number): Promise<string | null>
  drain(): Uint8Array
  onLine(cb: (line: string) => void): void
  close(): Promise<void>
}

function stty(path: string, baud: number) {
  const r = Bun.spawnSync(['stty', '-F', path, String(baud), 'raw', '-echo', '-echoe', '-echok', '-echoctl', '-echoke', '-hupcl', 'cs8', '-cstopb', '-parenb', '-crtscts'], { stderr: 'pipe' })
  if (r.exitCode !== 0) throw new Error(`stty ${path}: ${r.stderr.toString().trim()}`)
}

const BRIDGE_GUARD_MS = 160

// The bridge enumerates as the S3's "USB JTAG/serial debug unit"; OPENNEK_BRIDGE=0/1 overrides
export function isBridge(path: string) {
  if (process.env.OPENNEK_BRIDGE) return process.env.OPENNEK_BRIDGE === '1'
  const byId = '/dev/serial/by-id'
  if (!existsSync(byId)) return false
  const real = realpathSync(path)
  return readdirSync(byId).some((name) => /USB_JTAG_serial_debug_unit/.test(name) && realpathSync(`${byId}/${name}`) === real)
}

export function openPort(path: string, baud: number): Port {
  stty(path, baud)
  const bridge = isBridge(path)
  let lastWrite = 0
  const fd = openSync(path, constants.O_RDWR | constants.O_NOCTTY)
  const stream = createReadStream('', { fd })
  let buf: number[] = []
  let lineBuf = ''
  let waiter: (() => void) | null = null
  const lineListeners: ((line: string) => void)[] = []
  stream.on('data', (chunk) => {
    const bytes = chunk as Buffer
    for (const b of bytes) buf.push(b)
    if (lineListeners.length) {
      lineBuf += bytes.toString('latin1')
      let i: number
      while ((i = lineBuf.indexOf('\n')) >= 0) {
        const line = lineBuf.slice(0, i).replace(/\r$/, '')
        lineBuf = lineBuf.slice(i + 1)
        for (const l of lineListeners) l(line)
      }
      buf = [] // line mode consumes the data
    }
    waiter?.()
  })
  const wait = (ms: number) =>
    new Promise<void>((resolve) => {
      const t = setTimeout(() => {
        waiter = null
        resolve()
      }, ms)
      waiter = () => {
        clearTimeout(t)
        waiter = null
        resolve()
      }
    })
  return {
    write(data) {
      lastWrite = Date.now()
      if (typeof data === 'string') writeSync(fd, data)
      else writeSync(fd, data)
    },
    async setBaud(b) {
      stty(path, b)
      if (!bridge) return
      // The command is only recognised after a quiet period; its "~~~OK" reply is discarded
      await Bun.sleep(Math.max(0, BRIDGE_GUARD_MS - (Date.now() - lastWrite)))
      writeSync(fd, `~~~OPENNEK:BAUD=${b}\n`)
      lastWrite = Date.now()
      await wait(100)
      await Bun.sleep(20)
      buf = []
    },
    async readByte(timeoutMs) {
      const end = Date.now() + timeoutMs
      while (!buf.length) {
        const left = end - Date.now()
        if (left <= 0) return null
        await wait(left)
      }
      return buf.shift()!
    },
    async readUntil(text, timeoutMs) {
      const end = Date.now() + timeoutMs
      let seen = ''
      for (;;) {
        while (buf.length) seen += String.fromCharCode(buf.shift()!)
        if (seen.includes(text)) return seen
        const left = end - Date.now()
        if (left <= 0) return null
        await wait(left)
      }
    },
    drain() {
      const out = new Uint8Array(buf)
      buf = []
      return out
    },
    onLine: (cb) => void lineListeners.push(cb),
    close: () =>
      new Promise((resolve) => {
        stream.once('close', () => resolve())
        stream.destroy()
      }),
  }
}
