// ホストのログ (古い順)
import type { ConsoleLine } from './api.gen'

// ホストが覚えている行数と同じ
export const HOST_LOG_LINES = 1000

export const appendLines = (shown: readonly ConsoleLine[], lines: readonly ConsoleLine[]): ConsoleLine[] =>
  [...shown, ...lines].slice(-HOST_LOG_LINES)

const key = (l: ConsoleLine) => `${l.time} ${l.line}`

// イベントに (再) 接続したとき: ホストが覚えている行と、読み込み中に届いた行のうちそこに無いもの
// ホストに届かなければ (fromHost が null) 表示中の行を残す
export function reloadLines(
  shown: readonly ConsoleLine[],
  fromHost: readonly ConsoleLine[] | null,
  queued: readonly ConsoleLine[],
): ConsoleLine[] {
  if (!fromHost) return appendLines(shown, queued)
  const kept = new Set(fromHost.map(key))
  return appendLines(fromHost, queued.filter((l) => !kept.has(key(l))))
}
