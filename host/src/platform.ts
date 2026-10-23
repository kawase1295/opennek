import { posix, win32 } from 'node:path'

// OS ごとの既定: データの置き場所とブラウザを開くコマンド
export function defaultDataDir(platform: NodeJS.Platform, env: NodeJS.ProcessEnv, home: string) {
  if (platform === 'win32') return win32.join(env.APPDATA || win32.join(home, 'AppData', 'Roaming'), 'opennek')
  if (platform === 'darwin') return posix.join(home, 'Library', 'Application Support', 'opennek')
  return posix.join(env.XDG_DATA_HOME || posix.join(home, '.local', 'share'), 'opennek')
}

export function browserCommand(platform: NodeJS.Platform, url: string): [string, string[]] {
  if (platform === 'win32') return ['cmd', ['/c', 'start', '', url]]
  if (platform === 'darwin') return ['open', [url]]
  return ['xdg-open', [url]]
}
