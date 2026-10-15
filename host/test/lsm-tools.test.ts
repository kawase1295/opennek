import { afterAll, expect, test } from 'bun:test'
import { constants, createReadStream, existsSync, mkdtempSync, openSync, writeSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { crc16, headerBlock, ymodemBlock } from '../tools/lsm-flash'
import { findBursts, plotHtml } from '../tools/rssi-capture'

test('crc16 matches the XMODEM check value', () => {
  expect(crc16(new TextEncoder().encode('123456789'))).toBe(0x31c3)
})

test('ymodem blocks', () => {
  const h = headerBlock('fw.bin', 23316)
  expect(h.length).toBe(133)
  expect([h[0], h[1], h[2]]).toEqual([0x01, 0x00, 0xff])
  expect(new TextDecoder().decode(h.subarray(3, 3 + 13))).toBe('fw.bin\x0023316\x00')
  const d = ymodemBlock(1, new Uint8Array([1, 2, 3]), 1024)
  expect(d.length).toBe(1029)
  expect([d[0], d[1], d[2], d[3], d[6]]).toEqual([0x02, 1, 0xfe, 1, 0x1a])
})

// Pulse-shaped DBPSK envelope: a dip at every phase reversal
function dbpskEnvelope(bits: number[], rate: number, peakDbm: number, floorDbm: number) {
  const perBit = rate / 100
  const lead = rate * 0.3
  const out = new Float32Array(lead * 2 + bits.length * perBit).fill(floorDbm)
  bits.forEach((flip, k) => {
    for (let i = 0; i < perBit; i++) {
      // amplitude dips to ~0 at the start of a bit whose phase flips
      const a = flip ? Math.abs(Math.sin((Math.PI * (i + 0.5)) / perBit)) : 1
      const p = 10 * Math.log10(10 ** (peakDbm / 10) * a * a + 10 ** (floorDbm / 10))
      out[lead + k * perBit + i] = p + (Math.random() - 0.5)
    }
  })
  for (let i = 0; i < out.length; i++) if (out[i] === floorDbm) out[i] = floorDbm + (Math.random() - 0.5) * 2
  return out
}

test('findBursts sees one burst per DBPSK frame despite the dips', () => {
  const rate = 2000
  const bits = Array.from({ length: 200 }, () => (Math.random() < 0.5 ? 1 : 0))
  const a = dbpskEnvelope(bits, rate, -40, -110)
  const b = dbpskEnvelope(bits, rate, -55, -110)
  const all = new Float32Array(a.length + b.length)
  all.set(a)
  all.set(b, a.length)
  const bursts = findBursts(all, rate)
  expect(bursts).toHaveLength(2)
  expect((bursts[0]!.end - bursts[0]!.start) / rate).toBeCloseTo(2.0, 1)
  expect(bursts[0]!.peakDbm).toBeGreaterThan(-42)
  expect(plotHtml(all, rate, bursts, {})).toContain('Burst 2')
})

// A fake SJI IAP bootloader on one side of a pty pair; lsm-flash.ts drives the other side
const hasSocat = Bun.which('socat') !== null
const dir = mkdtempSync(join(tmpdir(), 'opennek-iap-'))
const devSide = join(dir, 'dev')
const hostSide = join(dir, 'host')
const socat = hasSocat ? Bun.spawn(['socat', `pty,raw,echo=0,link=${devSide}`, `pty,raw,echo=0,link=${hostSide}`], { stderr: 'ignore' }) : null
afterAll(() => socat?.kill())

test.skipIf(!hasSocat)('lsm-flash sends an image through the IAP ymodem dialog', async () => {
  for (let i = 0; i < 50 && !(existsSync(devSide) && existsSync(hostSide)); i++) await Bun.sleep(20)
  const fd = openSync(devSide, constants.O_RDWR | constants.O_NOCTTY)
  const rs = createReadStream('', { fd })
  let pending: number[] = []
  let received: number[] = []
  type State = 'menu' | 'header' | 'data' | 'final' | 'done'
  let state = 'menu' as State
  let size = 0
  let eots = 0
  const send = (b: string | number[]) => void (typeof b === 'string' ? writeSync(fd, b) : writeSync(fd, new Uint8Array(b)))
  // like the real bootloader, keep asking for a transfer until the header arrives
  const askCrc = setInterval(() => state === 'header' && send('C'), 1000)
  rs.on('data', (chunk) => {
    pending.push(...(chunk as Buffer))
    for (;;) {
      if (state === 'menu') {
        if (!pending.includes(0x31)) return
        pending = []
        state = 'header'
        send('\r\nWaiting for the file to be sent ... (press \'a\' to abort)\r\n')
        send('C')
        return
      }
      if (pending[0] === 0x04 && (state === 'data' || state === 'final')) {
        pending.shift()
        eots++
        if (eots === 1) send([0x15])
        else {
          send([0x06, 0x43])
          state = 'final'
        }
        continue
      }
      const len = pending[0] === 0x01 ? 128 : pending[0] === 0x02 ? 1024 : 0
      if (!len || pending.length < len + 5) return
      const block = pending.splice(0, len + 5)
      const body = new Uint8Array(block.slice(3, 3 + len))
      if (crc16(body) !== ((block[3 + len]! << 8) | block[4 + len]!)) {
        send([0x15])
        continue
      }
      if (state === 'header') {
        const text = new TextDecoder().decode(body)
        size = Number(text.split('\0')[1])
        send([0x06, 0x43])
        state = 'data'
      } else if (state === 'final') {
        send([0x06])
        send(`\r\nProgramming Completed Successfully!\r\nName: fw.bin\r\nSize: ${size} Bytes\r\n`)
        state = 'done'
      } else {
        received.push(...body)
        send([0x06])
      }
    }
  })

  const image = new Uint8Array(3000)
  new DataView(image.buffer).setUint32(0, 0x20010000, true)
  new DataView(image.buffer).setUint32(4, 0x08006415, true)
  for (let i = 8; i < image.length; i++) image[i] = (i * 7) & 0xff
  const file = join(dir, 'fw.bin')
  await Bun.write(file, image)
  const proc = Bun.spawn(['bun', join(import.meta.dir, '../tools/lsm-flash.ts'), hostSide, file], { stdout: 'pipe', stderr: 'pipe' })
  const code = await proc.exited
  const out = (await new Response(proc.stdout).text()) + (await new Response(proc.stderr).text())
  clearInterval(askCrc)
  rs.destroy()
  expect(out).toContain('Done.')
  expect(code).toBe(0)
  expect(state).toBe('done')
  expect(size).toBe(image.length)
  expect(new Uint8Array(received.slice(0, size))).toEqual(image)
}, 20000)
