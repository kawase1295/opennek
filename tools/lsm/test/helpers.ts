import { type ChildProcess, spawn, spawnSync } from 'node:child_process'

export { setTimeout as sleep } from 'node:timers/promises'

export const hasCommand = (cmd: string) => spawnSync('sh', ['-c', `command -v ${cmd}`]).status === 0

// Two linked ptys: one side for the fake device, the other for the code under test
export const ptyPair = (a: string, b: string): ChildProcess =>
  spawn('socat', [`pty,raw,echo=0,link=${a}`, `pty,raw,echo=0,link=${b}`], { stdio: 'ignore' })

// Runs a command to completion and returns its exit code with stdout and stderr together
export function run(cmd: string, args: string[]) {
  return new Promise<{ code: number | null; out: string }>((resolve) => {
    const p = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'] })
    let out = ''
    p.stdout.on('data', (d) => (out += d))
    p.stderr.on('data', (d) => (out += d))
    p.on('close', (code) => resolve({ code, out }))
  })
}
