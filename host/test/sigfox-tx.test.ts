import type { ChildProcess } from 'node:child_process'
import { afterAll, expect, test } from 'vitest'
import { constants, existsSync, mkdtempSync, openSync, writeSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { ReadStream as TtyReadStream } from 'node:tty'
import { SigfoxRadio } from '../src/radio'
import { hasCommand, ptyPair, sleep } from './helpers'

// A fake streamer on one side of a pty pair answers like firmware-lsm100a 0.3.0; SigfoxRadio drives the other side
const hasSocat = hasCommand('socat')
const dir = mkdtempSync(join(tmpdir(), 'opennek-tx-'))
const socats: ChildProcess[] = []
afterAll(() => socats.forEach((s) => s.kill()))

const frame = Uint8Array.from(Buffer.from('d1232ec88a6a34718ab5d1508837e4', 'hex'))

async function setup(name: string, fw: string, answer: (cmd: string) => string | null) {
  const fwPath = join(dir, `${name}-fw`)
  const hostPath = join(dir, `${name}-host`)
  socats.push(ptyPair(fwPath, hostPath))
  for (let i = 0; i < 50 && !(existsSync(fwPath) && existsSync(hostPath)); i++) await sleep(20)
  const fd = openSync(fwPath, constants.O_RDWR | constants.O_NOCTTY)
  const commands: string[] = []
  let buf = ''
  let tx = 0
  const status = () => `I fw=${fw} f=923200000 bw=10 sr=2000 boost=1 cfo=1 pd=4 tx=${tx}\n`
  const stream = new TtyReadStream(fd)
  stream.setEncoding('latin1')
  stream.on('error', () => {}) // EIO once socat is gone
  stream.on('data', (chunk) => {
    buf += chunk
    let i: number
    while ((i = buf.indexOf('\n')) >= 0) {
      const cmd = buf.slice(0, i)
      buf = buf.slice(i + 1)
      commands.push(cmd)
      if (cmd.startsWith('E')) tx = Number(cmd.slice(1))
      const reply = cmd.startsWith('T') ? answer(cmd) : status()
      if (reply !== null) writeSync(fd, cmd.startsWith('T') ? `D ${reply}\n` : reply)
    }
  })
  const radio = new SigfoxRadio(() => ({ port: hostPath, freq: 923_200_000 }))
  const logs: string[] = []
  radio.onLog((l) => logs.push(l))
  radio.start()
  for (let i = 0; i < 100 && radio.status().state !== 'connected'; i++) await sleep(20)
  return { radio, commands, logs }
}

const downlink = (deadline = Date.now() + 5000) => ({ deviceId: '0A1B2C3D', counter: 1234, freqHz: 922_183_000, frame, deadline })

test.skipIf(!hasSocat)('a downlink is allowed, sent and disallowed again', async () => {
  const { radio, commands, logs } = await setup('ok', '0.3.0', () => 'ok ms=281')
  expect(await radio.transmit(downlink())).toBe(true)
  await sleep(50)
  expect(commands.slice(commands.indexOf('E1'))).toEqual(['E1', 'T922183000,10,D1232EC88A6A34718AB5D1508837E4', 'E0'])
  expect(logs).toContain('sigfox: TX 922.183000 MHz 10 dBm: ok ms=281')
  await radio.stop()
})

test.skipIf(!hasSocat)('a busy channel is tried again until it is free', async () => {
  let n = 0
  const { radio, commands } = await setup('busy', '0.3.0', () => (++n < 3 ? 'busy rssi=-71.5' : 'ok ms=281'))
  expect(await radio.transmit(downlink())).toBe(true)
  expect(commands.filter((c) => c.startsWith('T'))).toHaveLength(3)
  await radio.stop()
})

test.skipIf(!hasSocat)('a busy channel is given up at the deadline', async () => {
  const { radio, commands, logs } = await setup('late', '0.3.0', () => 'busy rssi=-60.0')
  expect(await radio.transmit(downlink(Date.now() + 500))).toBe(false)
  await sleep(50)
  expect(commands.at(-1)).toBe('E0')
  expect(logs.at(-1)).toBe("sigfox: TX not sent: the device's receive window has closed")
  await radio.stop()
})

test.skipIf(!hasSocat)('a refusal from the firmware is not retried', async () => {
  const { radio, commands } = await setup('range', '0.3.0', () => 'range')
  expect(await radio.transmit(downlink())).toBe(false)
  expect(commands.filter((c) => c.startsWith('T'))).toHaveLength(1)
  await radio.stop()
})

test.skipIf(!hasSocat)('no reply counts as not sent', async () => {
  const { radio } = await setup('silent', '0.3.0', () => null)
  expect(await radio.transmit(downlink())).toBe(false)
  await radio.stop()
})

test.skipIf(!hasSocat)('an older streamer is not asked to transmit', async () => {
  const { radio, commands, logs } = await setup('old', '0.2.0', () => 'ok ms=281')
  expect(await radio.transmit(downlink())).toBe(false)
  expect(commands.some((c) => c.startsWith('T') || c.startsWith('E'))).toBe(false)
  expect(logs.at(-1)).toContain('firmware-lsm100a 0.3.0 or later needed (fw=0.2.0)')
  await radio.stop()
})

test('without a receiver nothing is sent', async () => {
  const radio = new SigfoxRadio(() => ({ port: '', freq: 923_200_000 }))
  expect(await radio.transmit(downlink())).toBe(false)
})
