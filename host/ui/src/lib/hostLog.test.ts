import { expect, test } from 'vitest'
import { HOST_LOG_LINES, appendLines, reloadLines } from './hostLog'

const line = (time: number, text = `line ${time}`) => ({ time, line: text })

test('the log keeps the newest lines', () => {
  const many = Array.from({ length: HOST_LOG_LINES + 5 }, (_, i) => line(i))
  const kept = appendLines([], many)
  expect(kept).toHaveLength(HOST_LOG_LINES)
  expect(kept[0]).toEqual(line(5))
})

test("on reconnect: the host's lines, then the ones that came meanwhile and are not among them", () => {
  const fromHost = [line(1), line(2)]
  const queued = [line(2), line(3)]
  expect(reloadLines([line(0)], fromHost, queued)).toEqual([line(1), line(2), line(3)])
})

test('when the host cannot be reached, what is shown stays and the queue is added', () => {
  expect(reloadLines([line(0)], null, [line(3)])).toEqual([line(0), line(3)])
})
