// メッセージの一覧 (新しい順)
import type { Message } from './api.gen'

// 一覧に m を入れるか置き換える。デバイスで絞っているとき (device) はほかのデバイスのものを入れない
export function upsertMessage(list: readonly Message[], m: Message, device: string): Message[] {
  if (device && m.deviceId !== device) return [...list]
  const rest = list.filter((x) => x.id !== m.id)
  const at = rest.findIndex((x) => x.id < m.id)
  return at < 0 ? [...rest, m] : [...rest.slice(0, at), m, ...rest.slice(at)]
}
