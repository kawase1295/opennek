// Flash a firmware image into the SEONG JI LSM100A through its IAP bootloader (Ymodem, 115200 bps).
//   bun tools/lsm-flash.ts /dev/ttyACM0 firmware.bin
// The module is reset automatically when it runs the stock AT firmware (ATZ at 9600 bps) or the
// OpenNEK RSSI streamer ("X"). Otherwise reset it by hand while "Waiting for the bootloader" is shown.
import { readFileSync } from 'node:fs'
import { basename } from 'node:path'
import { isBridge, openPort, type Port } from './serial'

const SOH = 0x01
const STX = 0x02
const EOT = 0x04
const ACK = 0x06
const NAK = 0x15
const CAN = 0x18
const CRC_REQ = 0x43 // 'C'

export function crc16(data: Uint8Array) {
  let crc = 0
  for (const b of data) {
    crc ^= b << 8
    for (let i = 0; i < 8; i++) crc = crc & 0x8000 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff
  }
  return crc
}

export function ymodemBlock(seq: number, data: Uint8Array, size: 128 | 1024, pad = 0x1a) {
  const block = new Uint8Array(3 + size + 2)
  block[0] = size === 128 ? SOH : STX
  block[1] = seq & 0xff
  block[2] = ~seq & 0xff
  const body = new Uint8Array(size).fill(pad)
  body.set(data.subarray(0, size))
  block.set(body, 3)
  const crc = crc16(body)
  block[3 + size] = crc >> 8
  block[4 + size] = crc & 0xff
  return block
}

export function headerBlock(name: string, size: number) {
  return ymodemBlock(0, new TextEncoder().encode(`${name}\0${size}\0`), 128, 0)
}

async function sendBlock(port: Port, block: Uint8Array, what: string) {
  for (let attempt = 0; attempt < 10; attempt++) {
    port.write(block)
    // Stray 'C's (CRC requests queued up before the transfer started) are ignored; resend on NAK or timeout
    const end = Date.now() + 5000
    let r: number | null = null
    while (Date.now() < end) {
      r = await port.readByte(end - Date.now())
      if (r !== CRC_REQ) break
    }
    if (process.env.YMODEM_DEBUG && r !== ACK) console.error(`\n${what} attempt ${attempt}: got ${r === null ? 'timeout' : `0x${r.toString(16)}`}`)
    if (r === ACK) return
    if (r === CAN) throw new Error(`receiver cancelled at ${what}`)
  }
  throw new Error(`no ACK for ${what}`)
}

async function waitFor(port: Port, byte: number, ms: number) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    const r = await port.readByte(end - Date.now())
    if (r === byte) return true
  }
  return false
}

export async function ymodemSend(
  port: Port,
  name: string,
  data: Uint8Array,
  progress: (sent: number) => void,
  crcRequested = false,
  blockSize: 128 | 1024 = 128,
) {
  if (!crcRequested && !(await waitFor(port, CRC_REQ, 10000))) throw new Error('receiver did not request a transfer')
  await sendBlock(port, headerBlock(name, data.length), 'header')
  if (!(await waitFor(port, CRC_REQ, 5000))) throw new Error('receiver did not start after header')
  // 128-byte blocks by default: USB-UART bridges with small buffers drop bytes of 1 KB blocks
  const size = blockSize
  let seq = 1
  for (let off = 0; off < data.length; off += size, seq++) {
    await sendBlock(port, ymodemBlock(seq, data.subarray(off, off + size), size), `block ${seq}`)
    progress(Math.min(off + size, data.length))
  }
  port.write(new Uint8Array([EOT]))
  let r = await port.readByte(5000)
  if (r === NAK) {
    port.write(new Uint8Array([EOT]))
    r = await port.readByte(5000)
  }
  if (r !== ACK) throw new Error('no ACK for EOT')
  if (await waitFor(port, CRC_REQ, 3000)) await sendBlock(port, ymodemBlock(0, new Uint8Array(), 128, 0), 'end block')
}

const PROMPT = 'Waiting for the file to be sent'
// The streamer's hex sample lines ("R0123:CCC...") can contain "CCC" too: drop them, and the tail of a line
// that was already being received, before looking for the bootloader
export function bootloaderOutput(seen: string) {
  return seen.replace(/^[R0-9A-F:]*\n/, '').replace(/R[0-9A-F]{4}:[0-9A-F]*\n?/g, '')
}

// Resolves once the bootloader prints its prompt; it is then waiting for the Ymodem header
async function enterBootloader(port: Port, bridge: boolean): Promise<void> {
  if (bridge) {
    // OpenNEK streamer: reset it with "X" and knock from here. The bridge's IAP command is too late for it:
    // it spends ~130 ms at 9600 bps for the AT firmware before knocking, and a KNOCK command needs 150 ms of
    // silence first
    await Bun.sleep(200)
    port.drain()
    port.write('\nX\n')
    let seen = ''
    for (const start = Date.now(); Date.now() - start < 1500; ) {
      port.write('1')
      await Bun.sleep(10)
      seen += new TextDecoder('latin1').decode(port.drain())
      // Stop at the prompt: further bytes would land in the Ymodem receiver
      if (bootloaderOutput(seen).includes(PROMPT)) return
    }
    // Stock AT firmware: the bridge resets the module and catches the 0.5 s bootloader window itself
    await Bun.sleep(200)
    port.drain()
    port.write('~~~OPENNEK:IAP\n')
  } else {
    // Stock AT firmware: soft reset at 9600; OpenNEK streamer: "X" at 115200
    await port.setBaud(9600)
    port.write('ATZ\r') // the AT firmware expects CR only
    await Bun.sleep(30)
    await port.setBaud(115200)
    port.write('\nX\n') // the newline ends any partial command, which would swallow the X
  }
  console.log('Waiting for the bootloader (reset the LSM100A now if nothing happens)...')
  // The prompt can arrive split across several polls, so keep everything seen so far
  let seen = ''
  const polls = Number(process.env.BOOT_WAIT_S ?? 30) * 20
  for (let i = 0; i < polls; i++) {
    if (!bridge) port.write('1')
    // Through the bridge, let it knock (it is fast enough for the 0.5 s window after a manual reset)
    else if (i % 200 === 40 && !seen.includes('~~~OK knock=seen') && !seen.includes('~~~OK iap=seen')) port.write('~~~OPENNEK:KNOCK=9000\n')
    await Bun.sleep(50)
    const got = new TextDecoder('latin1').decode(port.drain())
    if (got && process.env.YMODEM_DEBUG) console.error(`rx ${JSON.stringify(got)}`)
    seen += got
    // The prompt, or repeated CRC requests when it is already waiting from an earlier attempt
    const out = bootloaderOutput(seen)
    if (out.includes(PROMPT) || out.includes('CCC')) {
      // Through the bridge, wait for its "~~~OK" line: until then it may still be knocking
      for (let k = 0, o = out; bridge && k < 200 && !/~~~OK (knock|iap)=/.test(o.slice(o.search(/Waiting|CCC/))); k++) {
        await Bun.sleep(50)
        seen += new TextDecoder('latin1').decode(port.drain())
        o = bootloaderOutput(seen)
      }
      // Let the bootloader settle, then drop the backlog of CRC requests
      await Bun.sleep(300)
      // Sample lines still arriving mean the streamer is running: it was not the bootloader
      if (/R[0-9A-F]{4}:/.test(new TextDecoder('latin1').decode(port.drain()))) {
        seen = ''
        continue
      }
      return
    }
    if (seen.length > 4096) seen = seen.slice(-256)
  }
  throw new Error(`bootloader did not answer within ${polls / 20} s`)
}

if (import.meta.main) {
  const [path, file] = process.argv.slice(2)
  if (!path || !file) {
    console.error('usage: bun tools/lsm-flash.ts <serial port> <firmware.bin>')
    process.exit(2)
  }
  const image = new Uint8Array(readFileSync(file))
  const sp = new DataView(image.buffer).getUint32(0, true)
  const reset = new DataView(image.buffer).getUint32(4, true)
  if (sp >>> 24 !== 0x20 || reset >>> 0 < 0x08002000 || reset >>> 0 >= 0x08040000) {
    console.error(`${file} does not look like an LSM100A application image (sp=0x${sp.toString(16)} reset=0x${reset.toString(16)}); it must be linked at 0x08002000`)
    process.exit(1)
  }
  const port = openPort(path, 115200)
  try {
    await enterBootloader(port, isBridge(path))
    console.log(`Sending ${basename(file)} (${image.length} bytes)`)
    // The SJI bootloader only sends 'C' sporadically while idle, so start with the header right away
    await ymodemSend(port, basename(file), image, (sent) => process.stdout.write(`\r  ${Math.round((sent / image.length) * 100)}%`), true)
    process.stdout.write('\n')
    const tail = await port.readUntil('Size:', 5000)
    console.log(tail ? 'Done. The module is booting the new firmware.' : 'Transfer finished (no "Size:" confirmation seen).')
  } catch (e) {
    console.error(`\nerror: ${(e as Error).message}`)
    process.exitCode = 1
  } finally {
    await port.close()
  }
}
