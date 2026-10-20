import { constants, createReadStream, existsSync, openSync, readdirSync, realpathSync, writeSync } from 'node:fs'
import { Gateway } from './gateway'

// Linux のシリアルポートを stty で raw 115200 にして直接読み書きする (ネイティブアドオン不要)
const BAUD = 115200
const RETRY_MS = 2000
const HELLO_TIMEOUT_MS = 3000
const LIKELY_ESP32 = /espressif|cp210|silicon_labs|ch34|1a86|wch|usb_jtag|usb-serial|ftdi/i

export type SerialPortInfo = { path: string; label: string }

export function listSerialPorts(): SerialPortInfo[] {
  const ports: SerialPortInfo[] = []
  const seen = new Set<string>()
  const byId = '/dev/serial/by-id'
  if (existsSync(byId)) {
    for (const name of readdirSync(byId).sort()) {
      try {
        const path = realpathSync(`${byId}/${name}`)
        seen.add(path)
        ports.push({ path, label: name })
      } catch {}
    }
  }
  for (const name of readdirSync('/dev').sort()) {
    if (!/^tty(ACM|USB)\d+$/.test(name)) continue
    const path = `/dev/${name}`
    if (!seen.has(path)) ports.push({ path, label: name })
  }
  return ports
}

// exclude: a port used by something else (the Sigfox receiver), compared after resolving symlinks
export function pickSerialPort(exclude: string[] = []): string | null {
  const real = (p: string) => {
    try {
      return realpathSync(p)
    } catch {
      return p
    }
  }
  const skip = new Set(exclude.filter(Boolean).map(real))
  const ports = listSerialPorts().filter((p) => !skip.has(p.path))
  return (ports.find((p) => LIKELY_ESP32.test(p.label)) ?? ports[0])?.path ?? null
}

export type LinePort = { write(line: string): void; close(): Promise<void> }

// raw 115200 にしたポートを行単位で読む。stty や open の失敗は例外 (権限不足ならヒント付き)
export function openLinePort(port: string, onLine: (line: string) => void, onError: (e: unknown) => void): LinePort {
  const stty = Bun.spawnSync(['stty', '-F', port, String(BAUD), 'raw', '-echo', '-echoe', '-echok', '-echoctl', '-echoke', '-hupcl', 'cs8', '-cstopb', '-parenb', '-crtscts'], { stderr: 'pipe' })
  if (stty.exitCode !== 0) {
    const err = stty.stderr.toString().trim()
    const hint = /permission denied/i.test(err) ? ' — run `sudo usermod -aG dialout $USER` and log in again' : ''
    throw new Error(`${err}${hint}`)
  }
  const fd = openSync(port, constants.O_RDWR | constants.O_NOCTTY)
  let buf = ''
  // fd はストリームに持たせる (Bun は autoClose: false でも destroy 時に fd を閉じるため)
  const stream = createReadStream('', { fd, encoding: 'utf8' })
  let open = true
  stream.on('data', (chunk) => {
    buf += chunk
    let i: number
    while ((i = buf.indexOf('\n')) >= 0) {
      onLine(buf.slice(0, i))
      buf = buf.slice(i + 1)
    }
    if (buf.length > 4096) buf = ''
  })
  stream.on('error', (e) => open && onError(e))
  stream.on('end', () => open && onError(new Error('port closed')))
  return {
    write(line) {
      if (open) writeSync(fd, `${line}\n`)
    },
    // fd が実際に閉じるまで待てるようにする (fd 番号の再利用で別のファイルを閉じないように)
    close() {
      if (!open) return Promise.resolve()
      open = false
      return new Promise((resolve) => {
        stream.once('close', () => resolve())
        stream.destroy()
      })
    },
  }
}

export class SerialGateway extends Gateway {
  private port: LinePort | null = null
  private retry: Timer | null = null
  private helloTimer: Timer | null = null
  private stopped = true
  private closing: Promise<void> = Promise.resolve()

  // exclude: ポート自動検出で選ばないポート (Sigfox 受信機など)
  constructor(
    private portSetting: () => string,
    private exclude: () => string[] = () => [],
  ) {
    super('serial')
  }

  start() {
    this.stopped = false
    void this.connect()
  }

  async stop() {
    this.stopped = true
    if (this.retry) clearTimeout(this.retry)
    this.close()
    await this.closing
  }

  // 設定でポートが変わったときなどに繋ぎ直す
  reconnect() {
    this.close()
    if (!this.stopped) void this.connect()
  }

  protected writeLine(line: string) {
    if (!this.port) return
    try {
      this.port.write(line)
    } catch (e) {
      this.fail(e)
    }
  }

  private async connect() {
    await this.closing
    if (this.stopped || this.port) return
    const setting = this.portSetting()
    const path = setting === 'auto' ? pickSerialPort(this.exclude()) : setting
    if (!path) {
      this.setStatus({ state: 'disconnected', port: null, error: 'No serial port found (connect the gateway ESP32)' })
      return this.scheduleRetry()
    }
    this.setStatus({ state: 'connecting', port: path, error: undefined })
    try {
      this.port = openLinePort(path, (line) => this.handleLine(line), (e) => this.fail(e))
    } catch (e) {
      this.setStatus({ state: 'disconnected', error: e instanceof Error ? e.message : String(e) })
      return this.scheduleRetry()
    }
    this.log(`serial: opened ${path}`)
    // 起動直後の hello を取りこぼしても ping で取り直す
    const askHello = (n: number) => {
      if (!this.port || this.st.state === 'connected') return
      if (n === 0) {
        this.setStatus({ error: 'No reply from the gateway (flash the OpenNEK gateway firmware)' })
      }
      this.ping()
      this.helloTimer = setTimeout(() => askHello(n === 0 ? 0 : n - 1), HELLO_TIMEOUT_MS)
    }
    this.helloTimer = setTimeout(() => askHello(2), 300)
  }

  private fail(e: unknown) {
    if (!this.port) return
    this.log(`serial: ${e instanceof Error ? e.message : String(e)}`)
    this.close()
    this.setStatus({ state: 'disconnected', error: 'Lost the connection to the gateway' })
    this.scheduleRetry()
  }

  private close() {
    if (this.helloTimer) clearTimeout(this.helloTimer)
    if (this.port) this.closing = this.port.close()
    this.port = null
    if (this.st.state !== 'disconnected') this.setStatus({ state: 'disconnected' })
  }

  private scheduleRetry() {
    if (this.stopped) return
    if (this.retry) clearTimeout(this.retry)
    this.retry = setTimeout(() => void this.connect(), RETRY_MS)
  }
}
