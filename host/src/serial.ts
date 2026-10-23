import { basename } from 'node:path'
import { SerialPort } from 'serialport'

const BAUD = 115200

export type SerialPortInfo = { path: string; label: string }

type ListedPort = Awaited<ReturnType<typeof SerialPort.list>>[number] & { friendlyName?: string }

const byPath = (a: SerialPortInfo, b: SerialPortInfo) => a.path.localeCompare(b.path, 'en', { numeric: true })

// SerialPort.list() の結果を OS ごとに選ぶ。Linux は内蔵の ttyS*、macOS は Bluetooth などの USB でないポートを外す
export function toPortList(ports: ListedPort[], platform: NodeJS.Platform): SerialPortInfo[] {
  const named = (p: ListedPort) => [p.manufacturer, p.serialNumber].filter(Boolean).join(' ')
  if (platform === 'win32') return ports.map((p) => ({ path: p.path, label: p.friendlyName || named(p) || p.path })).sort(byPath)
  if (platform === 'darwin') {
    return ports
      .filter((p) => p.vendorId)
      // cu は相手の DCD を待たずに開ける発信用のノード
      .map((p) => ({ path: p.path.replace(/^\/dev\/tty\./, '/dev/cu.'), label: named(p) || basename(p.path) }))
      .sort(byPath)
  }
  return ports
    .filter((p) => p.vendorId || /\/tty(ACM|USB)\d+$/.test(p.path))
    .map((p) => ({ path: p.path, label: p.pnpId || named(p) || basename(p.path) }))
    .sort(byPath)
}

export const listSerialPorts = async () => toPortList(await SerialPort.list(), process.platform)

export type LinePort = { write(line: string): void; close(): Promise<void> }

// 115200 8N1 で開いて行単位で読む。開けなければ例外 (Linux で権限不足ならヒント付き)
export async function openLinePort(path: string, onLine: (line: string) => void, onError: (e: unknown) => void): Promise<LinePort> {
  const port = new SerialPort({ path, baudRate: BAUD, autoOpen: false })
  await new Promise<void>((resolve, reject) =>
    port.open((e) => {
      if (!e) return resolve()
      const hint = process.platform === 'linux' && /permission denied/i.test(e.message) ? ' — run `sudo usermod -aG dialout $USER` and log in again' : ''
      reject(new Error(`${e.message}${hint}`))
    }),
  )
  let open = true
  let buf = ''
  port.setEncoding('utf8')
  port.on('data', (chunk: string) => {
    // close は非同期なので、閉じると決めた後に届いた分は捨てる
    if (!open) return
    buf += chunk
    let i: number
    while ((i = buf.indexOf('\n')) >= 0) {
      onLine(buf.slice(0, i))
      buf = buf.slice(i + 1)
    }
    if (buf.length > 4096) buf = ''
  })
  port.on('error', (e) => open && onError(e))
  // 抜かれたときは error ではなく close (disconnected) で知らされる
  port.on('close', () => open && onError(new Error('port closed')))
  return {
    write(line) {
      if (open) port.write(`${line}\n`)
    },
    close() {
      if (!open) return Promise.resolve()
      open = false
      return new Promise((resolve) => (port.isOpen ? port.close(() => resolve()) : resolve()))
    },
  }
}
