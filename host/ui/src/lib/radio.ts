// ヘッダーの受信機の表示
import type { RadioStatus } from './api.gen'
import { mhz } from './format'
import { t } from './i18n'

export type RadioPill = { kind: 'connected' | 'connecting' | 'disconnected'; text: string; title: string }

export function radioPill(r: RadioStatus | null): RadioPill {
  const state = r?.state ?? 'off'
  const label = r?.state === 'connected' && r.freq ? mhz(r.freq, 1) : t(`radio.${state}`)
  return {
    kind: state === 'off' ? 'disconnected' : state,
    text: `Sigfox · ${label}`,
    title: r?.error || r?.port || '',
  }
}
