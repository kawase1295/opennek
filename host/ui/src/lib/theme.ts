// 画面の明暗。設定 (GET /api/settings の theme) に保存し、system は OS に従う
import type { Theme } from './api.gen'

// <html data-theme> に入れる値。null なら属性を外して prefers-color-scheme に任せる
export const themeAttribute = (theme: Theme): 'light' | 'dark' | null => (theme === 'system' ? null : theme)

export function applyTheme(theme: Theme): void {
  const v = themeAttribute(theme)
  if (v) document.documentElement.dataset.theme = v
  else delete document.documentElement.dataset.theme
}
