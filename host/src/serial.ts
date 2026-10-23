import { constants, createReadStream, existsSync, openSync, readdirSync, realpathSync, writeSync } from 'node:fs'

// Linux のシリアルポートを stty で raw 115200 にして直接読み書きする (ネイティブアドオン不要)
const BAUD = 115200

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
