// メッセージの認証とダウンリンクの状態、コールバックの結果を表すバッジ
// 言語は起動後に設定から決まるので、描画のたびに t() を呼ぶ
import type { Auth, CallbackLog, DownlinkStatus } from './api.gen'
import { t } from './i18n'

export type BadgeKind = 'ok' | 'warn' | 'bad' | 'muted' | 'sigfox'
export type Badge = { kind: BadgeKind; label: string }

const AUTH_KIND: Record<Auth, BadgeKind> = { ok: 'ok', bad: 'bad', public: 'muted', unverified: 'muted' }
const DL_KIND: Record<Exclude<DownlinkStatus, 'none'>, BadgeKind> = {
  pending: 'warn',
  sent: 'ok',
  acked: 'ok',
  failed: 'bad',
  nodata: 'muted',
  unsupported: 'muted',
}

export function authBadge(auth: Auth): Badge {
  return { kind: AUTH_KIND[auth], label: auth === 'ok' ? 'OK' : t(`auth.${auth}`) }
}

// ダウンリンクを要求していない (none) ときはバッジを出さない
export function dlBadge(status: DownlinkStatus): Badge | null {
  return status === 'none' ? null : { kind: DL_KIND[status], label: t(`dl.${status}`) }
}

export function logBadge(l: Pick<CallbackLog, 'ok' | 'status' | 'error'>): Badge {
  return { kind: l.ok ? 'ok' : 'bad', label: String(l.status ?? l.error ?? '') }
}
