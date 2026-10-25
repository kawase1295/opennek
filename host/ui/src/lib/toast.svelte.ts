// 画面下に一瞬出す通知
const SHOW_MS = 2500

export const toast = $state({ text: '', shown: false })

let timer: ReturnType<typeof setTimeout> | undefined
export function showToast(text: string): void {
  toast.text = text
  toast.shown = true
  clearTimeout(timer)
  timer = setTimeout(() => (toast.shown = false), SHOW_MS)
}

export const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err))
