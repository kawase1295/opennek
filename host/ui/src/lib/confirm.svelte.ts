// アプリ内の確認ダイアログ (ブラウザ標準の confirm() の代わり)
// ask() が開き、ConfirmDialog.svelte が answer() で答える
export type Question = {
  title: string
  // 補足の文。<code> などは使わない (テキストとして出す)
  body?: string
  confirm: string
  // 削除など取り消せない操作: 確定のボタンを赤にする
  danger?: boolean
}

export const pending = $state<{ question: Question | null }>({ question: null })
let resolve: ((yes: boolean) => void) | null = null

export function ask(question: Question): Promise<boolean> {
  // 開いたままの質問は「いいえ」で閉じる
  resolve?.(false)
  pending.question = question
  return new Promise((r) => (resolve = r))
}

export function answer(yes: boolean): void {
  const r = resolve
  resolve = null
  pending.question = null
  r?.(yes)
}
