// メッセージの認証とダウンリンクの状態を表すバッジ
// 言語は起動後に設定から決まるので、モジュールの読み込み時ではなく描画のたびに t() を呼ぶ
import { t } from './i18n.js'

const AUTH_CLASS = { ok: 'ok', bad: 'bad', public: 'muted', unverified: 'muted' }
const DL_CLASS = { pending: 'warn', sent: 'ok', acked: 'ok', failed: 'bad', nodata: 'muted', unsupported: 'muted' }

export function authBadge(auth) {
  if (!Object.hasOwn(AUTH_CLASS, auth)) return ''
  const label = auth === 'ok' ? 'OK' : t(`auth.${auth}`)
  return `<span class="badge ${AUTH_CLASS[auth]}">${label}</span>`
}

// ダウンリンクを要求していない (none) ときはバッジを出さない
export function dlBadge(status) {
  if (!Object.hasOwn(DL_CLASS, status)) return ''
  return `<span class="badge ${DL_CLASS[status]}">${t(`dl.${status}`)}</span>`
}
