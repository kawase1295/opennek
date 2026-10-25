// メッセージの認証とダウンリンクの状態、コールバックの結果を表すバッジ
// 正常は静かに、見てほしいものだけ目立たせる。言語は起動後に決まるので、描画のたびに t() を呼ぶ
import type { Auth, CallbackLog, DownlinkStatus } from './api.gen'
import { t } from './i18n'

export type Tone = 'neutral' | 'ok' | 'warn' | 'bad'
// soft: 薄い地 / quiet: 文字だけ / solid: 塗りつぶし / dot: 色の点付き / mono: 等幅 (HTTP の状態など)
export type Look = 'soft' | 'quiet' | 'solid' | 'dot' | 'mono'
export type Badge = { tone: Tone; look: Look; label: string }

const AUTH: Record<Auth, Omit<Badge, 'label'>> = {
  ok: { tone: 'ok', look: 'quiet' },
  bad: { tone: 'bad', look: 'solid' },
  public: { tone: 'neutral', look: 'soft' },
  unverified: { tone: 'neutral', look: 'soft' },
}
const DL_TONE: Record<Exclude<DownlinkStatus, 'none'>, Tone> = {
  pending: 'warn',
  sent: 'ok',
  acked: 'ok',
  failed: 'bad',
  nodata: 'neutral',
  unsupported: 'neutral',
}

export function authBadge(auth: Auth): Badge {
  return { ...AUTH[auth], label: auth === 'ok' ? 'OK' : t(`auth.${auth}`) }
}

// ダウンリンクを要求していない (none) ときはバッジを出さない
export function dlBadge(status: DownlinkStatus): Badge | null {
  return status === 'none' ? null : { tone: DL_TONE[status], look: 'dot', label: t(`dl.${status}`) }
}

// HTTP の状態は等幅で、成功は無彩色。届かなかったときはエラーの文
export function logBadge(l: Pick<CallbackLog, 'ok' | 'status' | 'error'>): Badge {
  const tone = l.ok ? 'neutral' : 'bad'
  return l.status === null
    ? { tone, look: 'soft', label: l.error ?? '' }
    : { tone, look: 'mono', label: String(l.status) }
}
