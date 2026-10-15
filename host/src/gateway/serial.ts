import { closeSync, constants, createReadStream, existsSync, openSync, readdirSync, realpathSync, writeSync, type ReadStream } from 'node:fs'
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

export function pickSerialPort(): string | null {
  const ports = listSerialPorts()
  return (ports.find((p) => LIKELY_ESP32.test(p.label)) ?? ports[0])?.path ?? null
}

export class SerialGateway extends Gateway {
  private fd: number | null = null
  private stream: ReadStream | null = null
  private retry: Timer | null = null
  private helloTimer: Timer | null = null
  private stopped = true
  private closing: Promise<void> = Promise.resolve()

  constructor(private portSetting: () => string) {
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
    if (this.fd === null) return
    try {
      writeSync(this.fd, `${line}\n`)
    } catch (e) {
      this.fail(e)
    }
  }

  private async connect() {
    await this.closing
    if (this.stopped || this.fd !== null) return
    const setting = this.portSetting()
    const port = setting === 'auto' ? pickSerialPort() : setting
    if (!port) {
      this.setStatus({ state: 'disconnected', port: null, error: 'シリアルポートが見つかりません (ゲートウェイ ESP32 を接続してください)' })
      return this.scheduleRetry()
    }
    this.setStatus({ state: 'connecting', port, error: undefined })
    const stty = Bun.spawnSync(['stty', '-F', port, String(BAUD), 'raw', '-echo', '-echoe', '-echok', '-echoctl', '-echoke', '-hupcl', 'cs8', '-cstopb', '-parenb', '-crtscts'], { stderr: 'pipe' })
    if (stty.exitCode !== 0) {
      const err = stty.stderr.toString().trim()
      const hint = /permission denied/i.test(err) ? ' — `sudo usermod -aG dialout $USER` して再ログインしてください' : ''
      this.setStatus({ state: 'disconnected', error: `${err}${hint}` })
      return this.scheduleRetry()
    }
    try {
      this.fd = openSync(port, constants.O_RDWR | constants.O_NOCTTY)
    } catch (e) {
      this.setStatus({ state: 'disconnected', error: String(e) })
      return this.scheduleRetry()
    }
    let buf = ''
    // fd はストリームに持たせる (Bun は autoClose: false でも destroy 時に fd を閉じるため)
    this.stream = createReadStream('', { fd: this.fd, encoding: 'utf8' })
    this.stream.on('data', (chunk) => {
      buf += chunk
      let i: number
      while ((i = buf.indexOf('\n')) >= 0) {
        this.handleLine(buf.slice(0, i))
        buf = buf.slice(i + 1)
      }
      if (buf.length > 4096) buf = ''
    })
    this.stream.on('error', (e) => this.fail(e))
    this.stream.on('end', () => this.fail(new Error('port closed')))
    this.log(`serial: opened ${port}`)
    // 起動直後の hello を取りこぼしても ping で取り直す
    const askHello = (n: number) => {
      if (this.fd === null || this.st.state === 'connected') return
      if (n === 0) {
        this.setStatus({ error: 'ゲートウェイから応答がありません (OpenNEK ゲートウェイのファームを書き込んでください)' })
      }
      this.ping()
      this.helloTimer = setTimeout(() => askHello(n === 0 ? 0 : n - 1), HELLO_TIMEOUT_MS)
    }
    this.helloTimer = setTimeout(() => askHello(2), 300)
  }

  private fail(e: unknown) {
    if (this.fd === null) return
    this.log(`serial: ${e instanceof Error ? e.message : String(e)}`)
    this.close()
    this.setStatus({ state: 'disconnected', error: 'ゲートウェイとの接続が切れました' })
    this.scheduleRetry()
  }

  private close() {
    if (this.helloTimer) clearTimeout(this.helloTimer)
    const stream = this.stream
    if (stream) {
      // fd が実際に閉じるまで次の接続を始めない (fd 番号の再利用で別のファイルを閉じないように)
      this.closing = new Promise((resolve) => stream.once('close', () => resolve()))
      stream.destroy()
    } else if (this.fd !== null) {
      try {
        closeSync(this.fd)
      } catch {}
    }
    this.stream = null
    this.fd = null
    if (this.st.state !== 'disconnected') this.setStatus({ state: 'disconnected' })
  }

  private scheduleRetry() {
    if (this.stopped) return
    if (this.retry) clearTimeout(this.retry)
    this.retry = setTimeout(() => void this.connect(), RETRY_MS)
  }
}
