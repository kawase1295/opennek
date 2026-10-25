// 画面全体の状態と、それを API から読み込む処理。表示の組み立ては lib/*.ts の関数とコンポーネントが受け持つ
import { SvelteSet } from 'svelte/reactivity'
import type { Callback, CallbackLog, ConsoleLine, Device, Event, Message, RadioStatus } from './api.gen'
import { api, subscribe } from './api'
import { appendLines, reloadLines } from './hostLog'
import { t } from './i18n'
import { upsertMessage } from './messages'

export const TABS = ['messages', 'devices', 'callbacks', 'log', 'settings'] as const
export type Tab = (typeof TABS)[number]

const PAGE = 100
const CALLBACK_LOGS = 50

class AppState {
  tab = $state<Tab>('messages')
  radio = $state<RadioStatus | null>(null)
  devices = $state<Device[]>([])
  callbacks = $state<Callback[]>([])
  callbackLogs = $state<CallbackLog[]>([])
  // 新しい順。deviceFilter が空でなければそのデバイスのものだけ
  messages = $state<Message[]>([])
  deviceFilter = $state('')
  hasMore = $state(false)
  // イベントで届いたメッセージ (行を光らせる)
  readonly live = new SvelteSet<number>()
  hostLog = $state<ConsoleLine[]>([])
  // ログを読み込んでいる間に届いた行
  #logQueue: ConsoleLine[] | null = null

  showTab(tab: Tab): void {
    this.tab = tab
    if (location.hash !== `#${tab}`) history.replaceState(null, '', `#${tab}`)
  }

  async loadDevices(): Promise<void> {
    this.devices = await api.devices()
  }

  async loadCallbacks(): Promise<void> {
    this.callbacks = await api.callbacks()
  }

  async loadCallbackLogs(): Promise<void> {
    this.callbackLogs = await api.callbackLogs({ limit: CALLBACK_LOGS })
  }

  async loadMessages(append = false): Promise<void> {
    const oldest = this.messages.at(-1)?.id
    const list = await api.messages({
      device: this.deviceFilter,
      ...(append && oldest !== undefined ? { before: oldest } : {}),
      limit: PAGE,
    })
    if (!append) {
      this.messages = []
      this.live.clear()
    }
    for (const m of list) this.messages = upsertMessage(this.messages, m, this.deviceFilter)
    this.hasMore = list.length === PAGE
  }

  deviceName(id: string): string {
    return this.devices.find((d) => d.id === id)?.name ?? ''
  }

  callbackName(id: number): string {
    const cb = this.callbacks.find((c) => c.id === id)
    return cb?.name || cb?.url || `#${id}`
  }

  // イベントに (再) 接続するたびに: ホストが覚えている行、読み込み中に届いた行の順
  async #reloadHostLog(): Promise<void> {
    this.#logQueue = []
    const lines = await api.hostLog().catch(() => null)
    const queued = this.#logQueue
    this.#logQueue = null
    this.hostLog = reloadLines(this.hostLog, lines, queued)
  }

  #onEvent(e: Event): void {
    switch (e.type) {
      case 'radio':
        this.radio = e.status
        break
      case 'console': {
        const line = { line: e.line, time: e.time }
        if (this.#logQueue) this.#logQueue.push(line)
        else this.hostLog = appendLines(this.hostLog, [line])
        break
      }
      case 'message':
        if (!this.devices.some((d) => d.id === e.message.deviceId)) void this.loadDevices()
        this.messages = upsertMessage(this.messages, e.message, this.deviceFilter)
        this.live.add(e.message.id)
        break
      case 'callbackLog':
        if (this.tab === 'callbacks') void this.loadCallbackLogs()
        break
    }
  }

  connect(): void {
    subscribe({
      open: () => void this.#reloadHostLog(),
      error: () =>
        (this.radio = {
          state: 'disconnected',
          port: null,
          error: t('radio.hostDown'),
          frames: 0,
          badFrames: 0,
          lostLines: 0,
        }),
      event: (e) => this.#onEvent(e),
    })
  }

  async start(): Promise<void> {
    const hash = location.hash.slice(1)
    this.showTab(TABS.find((tab) => tab === hash) ?? 'messages')
    this.radio = (await api.status()).radio
    await Promise.all([this.loadDevices(), this.loadCallbacks()])
    await this.loadMessages()
    this.connect()
  }
}

export const app = new AppState()
