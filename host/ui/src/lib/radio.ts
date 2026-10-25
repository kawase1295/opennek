// ヘッダーの受信機の表示
// 受信機を使わない (off) のは灰色、つながらないときだけ赤にする
import type { RadioStatus } from './api.gen'
import { mhz } from './format'
import { t } from './i18n'

export type RadioPill = {
  tone: 'off' | 'connecting' | 'connected' | 'error'
  label: string
  // 受信中は周波数、それ以外はポート
  detail: string
  title: string
}

const TONE = { off: 'off', connecting: 'connecting', connected: 'connected', disconnected: 'error' } as const

export function radioPill(r: RadioStatus | null): RadioPill {
  const state = r?.state ?? 'off'
  const freq = r?.state === 'connected' && r.freq ? mhz(r.freq, 1) : ''
  return {
    tone: TONE[state],
    label: t(`radio.${state}`),
    detail: freq || (r?.port ?? ''),
    title: r?.error || r?.port || '',
  }
}
