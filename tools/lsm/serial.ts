import { setTimeout as sleep } from 'node:timers/promises'
import { SerialPort } from 'serialport'

// Serial port for the CLI tools on top of serialport: raw bytes, a line mode and a changeable baud rate.
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

const BRIDGE_GUARD_MS = 160

// The bridge enumerates as the S3's "USB JTAG/serial debug unit" (303a:1001); OPENNEK_BRIDGE=0/1 overrides
export async function isBridge(path: string) {
  if (process.env.OPENNEK_BRIDGE) return process.env.OPENNEK_BRIDGE === '1'
  const port = (await SerialPort.list()).find((p) => p.path === path || p.path.replace(/^\/dev\/tty\./, '/dev/cu.') === path)
  return port?.vendorId?.toLowerCase() === '303a' && port.productId?.toLowerCase() === '1001'
}

export async function openPort(path: string, baud: number): Promise<Port> {
  const bridge = await isBridge(path)
  let lastWrite = 0
  const stream = new SerialPort({ path, baudRate: baud, autoOpen: false })
  await new Promise<void>((resolve, reject) => stream.open((e) => (e ? reject(new Error(`${path}: ${e.message}`)) : resolve())))
  const drained = () => new Promise<void>((resolve) => stream.drain(() => resolve()))
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
      stream.write(typeof data === 'string' ? Buffer.from(data, 'latin1') : data)
    },
    async setBaud(b) {
      await drained()
      await new Promise<void>((resolve, reject) => stream.update({ baudRate: b }, (e) => (e ? reject(e) : resolve())))
      if (!bridge) return
      // The command is only recognised after a quiet period; its "~~~OK" reply is discarded
      await sleep(Math.max(0, BRIDGE_GUARD_MS - (Date.now() - lastWrite)))
      stream.write(`~~~OPENNEK:BAUD=${b}\n`)
      await drained()
      lastWrite = Date.now()
      await wait(100)
      await sleep(20)
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
        if (!stream.isOpen) return resolve()
        void drained().then(() => stream.close(() => resolve()))
      }),
  }
}
