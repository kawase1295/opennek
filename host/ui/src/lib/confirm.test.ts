import { expect, test } from 'vitest'
import { answer, ask, pending } from './confirm.svelte'

test('a question waits for its answer', async () => {
  const yes = ask({ title: 'Delete?', confirm: 'Delete', danger: true })
  expect(pending.question?.title).toBe('Delete?')
  answer(true)
  expect(await yes).toBe(true)
  expect(pending.question).toBeNull()

  const no = ask({ title: 'Send?', confirm: 'Send' })
  answer(false)
  expect(await no).toBe(false)
})

test('a new question cancels the one still open', async () => {
  const first = ask({ title: 'A', confirm: 'OK' })
  const second = ask({ title: 'B', confirm: 'OK' })
  expect(await first).toBe(false)
  expect(pending.question?.title).toBe('B')
  answer(true)
  expect(await second).toBe(true)
})
