// 設定画面の受信機のポート
// sigfoxPort は、ポートが未設定なら "" (ホストが XIAO キットを自分で探す)、受信機を使わないなら null
import type { PortInfo, ReceiverPort } from './api.gen'
import { t } from './i18n'

// <select> の値は文字列なので、null をこの値で表す
export const NO_RECEIVER = 'off'

export type PortOption = { value: string; text: string }

export function portOptions(ports: readonly PortInfo[], saved: ReceiverPort): { options: PortOption[]; value: string } {
  const options: PortOption[] = [
    { value: '', text: t('set.findKit') },
    { value: NO_RECEIVER, text: t('set.notUsed') },
    ...ports.map((p) => ({ value: p.path, text: p.label.includes(p.path) ? p.label : `${p.label} (${p.path})` })),
  ]
  const value = saved ?? NO_RECEIVER
  // 保存したポートが今は挿さっていなくても選べるようにする
  if (!options.some((o) => o.value === value)) options.push({ value, text: value })
  return { options, value }
}

export const sigfoxPortSetting = (value: string): ReceiverPort => (value === NO_RECEIVER ? null : value)
