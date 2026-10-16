// OpenNEK Web UI (ビルド不要の素の JS)
const $ = (sel, root = document) => root.querySelector(sel)
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)]
const esc = (v) =>
  String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])
const fmtTime = (ms) => (ms ? new Date(ms).toLocaleString() : '—')
const spaced = (hex) => (hex ? hex.match(/../g).join(' ') : '—')

async function api(path, opts = {}) {
  const res = await fetch(`/api${path}`, {
    ...opts,
    headers: opts.body ? { 'Content-Type': 'application/json' } : undefined,
    body: opts.body ? JSON.stringify(opts.body) : undefined,
  })
  if (res.status === 204) return null
  const json = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`)
  return json
}

let toastTimer
function toast(msg) {
  const el = $('#toast')
  el.textContent = msg
  el.classList.add('show')
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => el.classList.remove('show'), 2500)
}

const state = { devices: [], callbacks: [], messages: new Map(), gateway: null, radio: null, settings: null }

// ---------------- tabs
function showTab(name) {
  $$('nav button').forEach((b) => b.classList.toggle('active', b.dataset.tab === name))
  $$('.tab').forEach((t) => t.classList.toggle('active', t.id === `tab-${name}`))
  if (location.hash !== `#${name}`) history.replaceState(null, '', `#${name}`)
  if (name === 'callbacks') loadLogs()
  if (name === 'settings') loadSettings()
}
$$('nav button').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.tab)))

// ---------------- gateway
function renderGateway(g) {
  state.gateway = g
  const pill = $('#gw')
  pill.className = `pill ${g.state}`
  const label = { connected: '接続中', connecting: '接続待ち', disconnected: '未接続' }[g.state]
  pill.textContent = g.kind === 'sim' ? `シミュレータ · ch ${g.channel ?? '-'}` : g.state === 'connected' ? `${label} · ch ${g.channel}` : label
  pill.title = g.error || g.port || ''
  $('#sim').classList.toggle('hidden', g.kind !== 'sim')
  $('#gw-detail').innerHTML = [
    ['状態', `${label}${g.error ? ` — ${esc(g.error)}` : ''}`],
    ['種類', g.kind === 'sim' ? 'シミュレータ' : 'USB シリアル'],
    ['ポート', esc(g.port ?? '—')],
    ['ファームウェア', esc(g.fw ?? '—')],
    ['無線', esc(g.radio ?? '—')],
    ['チャンネル', esc(g.channel ?? '—')],
    ['MAC', esc(g.mac ?? '—')],
  ]
    .map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`)
    .join('')
}

// ---------------- Sigfox receiver
function renderRadio(r) {
  state.radio = r
  const pill = $('#radio')
  pill.classList.toggle('hidden', !r || r.state === 'off')
  const label = { off: '無効', connected: '受信中', connecting: '応答待ち', disconnected: '未接続' }[r?.state ?? 'off']
  if (r && r.state !== 'off') {
    pill.className = `pill ${r.state}`
    pill.textContent = `Sigfox · ${r.state === 'connected' && r.freq ? `${(r.freq / 1e6).toFixed(1)} MHz` : label}`
    pill.title = r.error || r.port || ''
  }
  $('#radio-detail').innerHTML = (
    r && r.state !== 'off'
      ? [
          ['状態', `${label}${r.error ? ` — ${esc(r.error)}` : ''}`],
          ['ポート', esc(r.port ?? '—')],
          ['ファームウェア', esc(r.fw ?? '—')],
          ['周波数', r.freq ? `${(r.freq / 1e6).toFixed(3)} MHz` : '—'],
          ['サンプリング', r.rate ? `${r.rate} Hz` : '—'],
          ['受信フレーム', `${r.frames} (CRC エラー ${r.badFrames})`],
          ['最終受信', fmtTime(r.lastFrameAt)],
          ['欠落行', r.lostLines],
        ]
      : [['状態', '無効 (下の設定で受信機のポートを選ぶと有効になります)']]
  )
    .map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`)
    .join('')
}

// ---------------- messages
const msgBody = $('#msg-table tbody')
const DL_BADGE = {
  none: '',
  pending: '<span class="badge warn">待機中</span>',
  sent: '<span class="badge ok">送信済</span>',
  failed: '<span class="badge bad">失敗</span>',
  nodata: '<span class="badge muted">データなし</span>',
  unsupported: '<span class="badge muted">受信専用</span>',
}
const RADIO_BADGE = {
  espnow: '<span class="badge muted">ESP-NOW</span>',
  sigfox: '<span class="badge sigfox">Sigfox</span>',
}
const AUTH_BADGE = {
  ok: '<span class="badge ok">OK</span>',
  bad: '<span class="badge bad">鍵不一致</span>',
  unknown: '<span class="badge muted">未登録</span>',
  unverified: '<span class="badge muted">未検証</span>',
}

function deviceLabel(id) {
  const d = state.devices.find((x) => x.id === id)
  return d?.name ? `${esc(id)} <span class="muted">${esc(d.name)}</span>` : esc(id)
}

function messageRow(m) {
  const decoded = m.decoded
    ? `<span class="decoded">${esc(
        Object.entries(m.decoded)
          .map(([k, v]) => `${k}=${v}`)
          .join('  '),
      )}</span>`
    : ''
  const dl = m.ack ? `${DL_BADGE[m.downlinkStatus] ?? ''} ${m.downlink ? `<code>${spaced(m.downlink)}</code>` : ''}` : m.oob ? '<span class="badge muted">OOB</span>' : ''
  return `<td>${fmtTime(m.time)}</td><td>${RADIO_BADGE[m.radio] ?? ''}</td><td>${deviceLabel(m.deviceId)}</td><td>${m.seq}</td>
    <td class="data">${spaced(m.data)}${decoded}</td><td>${m.rssi} dBm</td><td>${m.nbFrames}/3</td>
    <td>${AUTH_BADGE[m.auth]}</td><td>${dl}</td>`
}

function upsertMessage(m, live) {
  const filter = $('#msg-device').value
  if (filter && m.deviceId !== filter) return
  state.messages.set(m.id, m)
  let tr = msgBody.querySelector(`tr[data-id="${m.id}"]`)
  if (!tr) {
    tr = document.createElement('tr')
    tr.dataset.id = m.id
    const firstOlder = [...msgBody.children].find((r) => Number(r.dataset.id) < m.id)
    msgBody.insertBefore(tr, firstOlder ?? null)
    if (live) tr.classList.add('new')
  }
  tr.innerHTML = messageRow(m)
  $('#msg-empty').classList.toggle('hidden', msgBody.children.length > 0)
}

async function loadMessages(append = false) {
  const dev = $('#msg-device').value
  const last = append ? Math.min(...state.messages.keys()) : undefined
  const params = new URLSearchParams({ limit: '100' })
  if (dev) params.set('device', dev)
  if (last) params.set('before', String(last))
  const list = await api(`/messages?${params}`)
  if (!append) {
    msgBody.innerHTML = ''
    state.messages.clear()
  }
  list.forEach((m) => upsertMessage(m, false))
  $('#msg-more').classList.toggle('hidden', list.length < 100)
  $('#msg-empty').classList.toggle('hidden', msgBody.children.length > 0)
  $('#msg-csv').href = `/api/messages.csv${dev ? `?device=${dev}` : ''}`
}

$('#msg-device').addEventListener('change', () => loadMessages())
$('#msg-more').addEventListener('click', () => loadMessages(true))
$('#msg-clear').addEventListener('click', async () => {
  const dev = $('#msg-device').value
  if (!confirm(dev ? `${dev} のメッセージを削除しますか?` : 'すべてのメッセージを削除しますか?')) return
  await api(`/messages${dev ? `?device=${dev}` : ''}`, { method: 'DELETE' })
  loadMessages()
})

msgBody.addEventListener('click', async (e) => {
  const tr = e.target.closest('tr')
  const m = tr && state.messages.get(Number(tr.dataset.id))
  if (!m) return
  $('#msg-detail').innerHTML = [
    ['ID', m.id],
    ['時刻', fmtTime(m.time)],
    ['デバイス', deviceLabel(m.deviceId)],
    ['無線', RADIO_BADGE[m.radio] ?? esc(m.radio)],
    m.radio === 'sigfox' ? ['受信', 'Sigfox 受信機 (認証は未検証)'] : ['送信元 MAC', esc(m.src)],
    ['Seq', m.seq],
    ['データ', `<code>${spaced(m.data)}</code>`],
    ['デコード', m.decoded ? `<code>${esc(JSON.stringify(m.decoded))}</code>` : '—'],
    ['RSSI', `${m.rssi} dBm`],
    ['受信フレーム', `${m.nbFrames}/3`],
    ['認証', AUTH_BADGE[m.auth]],
    ['ダウンリンク', m.ack ? `${DL_BADGE[m.downlinkStatus]} ${m.downlink ? `<code>${spaced(m.downlink)}</code>` : ''}` : '要求なし'],
  ]
    .map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`)
    .join('')
  const logs = await api(`/callback-logs?message=${m.id}`)
  $('#msg-logs').innerHTML = logs.length ? logs.map(logBlock).join('') : '<p class="muted">実行されたコールバックはありません。</p>'
  $('#msg-dialog').showModal()
})
$('#msg-dialog-close').addEventListener('click', () => $('#msg-dialog').close())

function logBlock(l) {
  const cb = state.callbacks.find((c) => c.id === l.callbackId)
  return `<div><strong>${esc(cb?.name || cb?.url || `#${l.callbackId}`)}</strong>
    ${l.ok ? '<span class="badge ok">' : '<span class="badge bad">'}${esc(l.status ?? l.error)}</span>
    <span class="muted">${l.durationMs} ms</span>${l.error && l.status ? ` <span class="muted">${esc(l.error)}</span>` : ''}
    ${l.response ? `<pre>${esc(l.response)}</pre>` : ''}</div>`
}

// ---------------- simulator
$('#sim-form').addEventListener('submit', async (e) => {
  e.preventDefault()
  const f = e.target
  const btn = $('button', f)
  btn.disabled = true
  $('#sim-result').textContent = f.ack.checked ? 'ダウンリンクを待っています…' : ''
  try {
    const r = await api('/sim/uplink', { method: 'POST', body: { deviceId: f.deviceId.value, data: f.data.value, ack: f.ack.checked } })
    $('#sim-result').textContent = `送信しました (seq ${r.seq})${f.ack.checked ? ` — ダウンリンク: ${r.downlink ? spaced(r.downlink) : '受信なし'}` : ''}`
    loadDevices()
  } catch (err) {
    $('#sim-result').textContent = err.message
  } finally {
    btn.disabled = false
  }
})

// ---------------- devices
async function loadDevices() {
  state.devices = await api('/devices')
  const body = $('#dev-table tbody')
  const DL = { none: '送らない', static: '固定', callback: 'コールバック' }
  body.innerHTML = state.devices
    .map(
      (d) => `<tr data-id="${esc(d.id)}">
        <td><code>${esc(d.id)}</code></td><td>${esc(d.name) || '<span class="muted">—</span>'}</td>
        <td>${d.key ? '<span class="badge ok">登録済</span>' : '<span class="badge muted">未登録</span>'}</td>
        <td>${DL[d.downlinkMode]}${d.downlinkMode === 'static' ? ` <code>${spaced(d.downlinkData)}</code>` : ''}</td>
        <td>${fmtTime(d.lastSeen)}</td><td>${d.lastSeq ?? '—'}</td>
        <td><button class="small" data-act="edit">編集</button> <button class="small danger" data-act="del">削除</button></td></tr>`,
    )
    .join('')
  $('#dev-empty').classList.toggle('hidden', state.devices.length > 0)
  const sel = $('#msg-device')
  const cur = sel.value
  sel.innerHTML =
    '<option value="">すべてのデバイス</option>' +
    state.devices.map((d) => `<option value="${esc(d.id)}">${esc(d.id)}${d.name ? ` (${esc(d.name)})` : ''}</option>`).join('')
  sel.value = cur
}

function openDevice(d) {
  const f = $('#dev-form')
  f.reset()
  f.id.value = d?.id ?? ''
  f.id.readOnly = !!d
  f.name.value = d?.name ?? ''
  f.key.value = d?.key ?? ''
  f.payloadConfig.value = d?.payloadConfig ?? ''
  f.downlinkMode.value = d?.downlinkMode ?? 'none'
  f.downlinkData.value = d?.downlinkData ?? ''
  $('#dev-error').textContent = ''
  syncDownlinkField()
  $('#dev-dialog').showModal()
}
function syncDownlinkField() {
  const f = $('#dev-form')
  const show = f.downlinkMode.value === 'static'
  $('[data-show="static"]', f).classList.toggle('hidden', !show)
  f.downlinkData.required = show
}
$('#dev-form').downlinkMode.addEventListener('change', syncDownlinkField)
$('#dev-add').addEventListener('click', () => openDevice(null))
$('#dev-table tbody').addEventListener('click', async (e) => {
  const btn = e.target.closest('button')
  if (!btn) return
  const d = state.devices.find((x) => x.id === btn.closest('tr').dataset.id)
  if (btn.dataset.act === 'edit') openDevice(d)
  if (btn.dataset.act === 'del' && confirm(`${d.id} を削除しますか? (メッセージは残ります)`)) {
    await api(`/devices/${d.id}`, { method: 'DELETE' })
    loadDevices()
  }
})
$('#dev-form').addEventListener('submit', async (e) => {
  if (e.submitter?.value !== 'save') return
  e.preventDefault()
  const f = e.target
  try {
    await api(`/devices/${f.id.value}`, {
      method: 'PUT',
      body: {
        name: f.name.value,
        key: f.key.value,
        payloadConfig: f.payloadConfig.value,
        downlinkMode: f.downlinkMode.value,
        downlinkData: f.downlinkMode.value === 'static' ? f.downlinkData.value : '',
      },
    })
    $('#dev-dialog').close()
    toast('保存しました')
    await loadDevices()
    loadMessages()
  } catch (err) {
    $('#dev-error').textContent = err.message
  }
})

// ---------------- callbacks
async function loadCallbacks() {
  state.callbacks = await api('/callbacks')
  $('#cb-list').innerHTML = state.callbacks
    .map(
      (c) => `<div class="card" data-id="${c.id}">
        <div><strong>${esc(c.name || `コールバック #${c.id}`)}</strong>
          ${c.enabled ? '<span class="badge ok">有効</span>' : '<span class="badge muted">無効</span>'}
          ${c.downlink ? '<span class="badge warn">ダウンリンク</span>' : ''}</div>
        <div class="url"><code>${esc(c.method)}</code> ${esc(c.url)}</div>
        <div class="muted">対象: ${esc(c.devices || 'すべて')}</div>
        <div class="actions"><button class="small" data-act="test">テスト送信</button>
          <button class="small" data-act="edit">編集</button>
          <button class="small danger" data-act="del">削除</button></div></div>`,
    )
    .join('')
  $('#cb-empty').classList.toggle('hidden', state.callbacks.length > 0)
}

async function loadLogs() {
  const logs = await api('/callback-logs?limit=50')
  $('#log-table tbody').innerHTML = logs
    .map((l) => {
      const cb = state.callbacks.find((c) => c.id === l.callbackId)
      return `<tr><td>${fmtTime(l.time)}</td><td>${esc(cb?.name || `#${l.callbackId}`)}</td><td>#${l.messageId}</td>
        <td>${l.ok ? `<span class="badge ok">${l.status}</span>` : `<span class="badge bad">${esc(l.status ?? l.error)}</span>`}</td>
        <td>${l.durationMs} ms</td><td class="mono">${esc((l.response ?? l.error ?? '').slice(0, 80))}</td></tr>`
    })
    .join('')
}

const DEFAULT_BODY = '{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi},"ack":{ack}}'

function openCallback(c) {
  const f = $('#cb-form')
  f.reset()
  f.dataset.id = c?.id ?? ''
  f.name.value = c?.name ?? ''
  f.method.value = c?.method ?? 'POST'
  f.url.value = c?.url ?? ''
  f.devices.value = c?.devices ?? ''
  f.headers.value = Object.entries(c?.headers ?? {})
    .map(([k, v]) => `${k}: ${v}`)
    .join('\n')
  f.contentType.value = c?.contentType ?? 'application/json'
  f.body.value = c?.body ?? DEFAULT_BODY
  f.downlink.checked = c?.downlink ?? false
  f.enabled.checked = c?.enabled ?? true
  $('#cb-error').textContent = ''
  $('#cb-dialog').showModal()
}
$('#cb-add').addEventListener('click', () => openCallback(null))
$$('#cb-form .vars code').forEach((code) =>
  code.addEventListener('click', () => {
    const ta = $('#cb-form').body
    ta.setRangeText(code.textContent, ta.selectionStart, ta.selectionEnd, 'end')
    ta.focus()
  }),
)
$('#cb-list').addEventListener('click', async (e) => {
  const btn = e.target.closest('button')
  if (!btn) return
  const c = state.callbacks.find((x) => x.id === Number(btn.closest('.card').dataset.id))
  if (btn.dataset.act === 'edit') openCallback(c)
  if (btn.dataset.act === 'del' && confirm('このコールバックを削除しますか?')) {
    await api(`/callbacks/${c.id}`, { method: 'DELETE' })
    loadCallbacks()
  }
  if (btn.dataset.act === 'test') {
    btn.disabled = true
    try {
      const { result } = await api(`/callbacks/${c.id}/test`, { method: 'POST' })
      toast(result.ok ? `成功: HTTP ${result.status} (${result.durationMs} ms)${result.downlink ? ` / downlink ${result.downlink}` : ''}` : `失敗: ${result.error}`)
    } catch (err) {
      toast(err.message)
    } finally {
      btn.disabled = false
    }
  }
})
$('#cb-form').addEventListener('submit', async (e) => {
  if (e.submitter?.value !== 'save') return
  e.preventDefault()
  const f = e.target
  const headers = {}
  for (const line of f.headers.value.split('\n')) {
    const i = line.indexOf(':')
    if (i > 0) headers[line.slice(0, i).trim()] = line.slice(i + 1).trim()
  }
  const body = {
    name: f.name.value,
    method: f.method.value,
    url: f.url.value,
    devices: f.devices.value,
    headers,
    contentType: f.contentType.value,
    body: f.body.value,
    downlink: f.downlink.checked,
    enabled: f.enabled.checked,
  }
  try {
    const id = f.dataset.id
    await api(id ? `/callbacks/${id}` : '/callbacks', { method: id ? 'PUT' : 'POST', body })
    $('#cb-dialog').close()
    toast('保存しました')
    loadCallbacks()
  } catch (err) {
    $('#cb-error').textContent = err.message
  }
})

// ---------------- settings
async function loadSettings() {
  const [s, ports] = await Promise.all([api('/settings'), api('/serial/ports')])
  state.settings = s
  const f = $('#settings-form')
  const opts = [{ path: 'auto', label: '自動検出' }, ...ports]
  if (!opts.some((p) => p.path === s.serialPort)) opts.push({ path: s.serialPort, label: s.serialPort })
  f.serialPort.innerHTML = opts.map((p) => `<option value="${esc(p.path)}">${esc(p.label)}${p.path !== 'auto' && p.label !== p.path ? ` (${esc(p.path)})` : ''}</option>`).join('')
  f.serialPort.value = s.serialPort
  f.serialPort.disabled = state.gateway?.kind === 'sim'
  const sfOpts = [{ path: '', label: '使わない' }, ...ports]
  if (!sfOpts.some((p) => p.path === s.sigfoxPort)) sfOpts.push({ path: s.sigfoxPort, label: s.sigfoxPort })
  f.sigfoxPort.innerHTML = sfOpts.map((p) => `<option value="${esc(p.path)}">${esc(p.label)}${p.path && p.label !== p.path ? ` (${esc(p.path)})` : ''}</option>`).join('')
  f.sigfoxPort.value = s.sigfoxPort
  f.sigfoxFreq.value = s.sigfoxFreq
  f.channel.value = s.channel
  f.dedupWindowMs.value = s.dedupWindowMs
  f.downlinkTimeoutMs.value = s.downlinkTimeoutMs
  f.acceptUnknownDevices.checked = s.acceptUnknownDevices
}
$('#settings-form').addEventListener('submit', async (e) => {
  e.preventDefault()
  const f = e.target
  try {
    await api('/settings', {
      method: 'PUT',
      body: {
        serialPort: f.serialPort.value,
        channel: Number(f.channel.value),
        dedupWindowMs: Number(f.dedupWindowMs.value),
        downlinkTimeoutMs: Number(f.downlinkTimeoutMs.value),
        acceptUnknownDevices: f.acceptUnknownDevices.checked,
        sigfoxPort: f.sigfoxPort.value,
        sigfoxFreq: Number(f.sigfoxFreq.value),
      },
    })
    toast('保存しました')
  } catch (err) {
    toast(err.message)
  }
})

// ---------------- live events
function connectEvents() {
  const es = new EventSource('/api/events')
  es.addEventListener('gateway', (e) => {
    const data = JSON.parse(e.data)
    renderGateway(data.status ?? data)
  })
  es.addEventListener('radio', (e) => renderRadio(JSON.parse(e.data).status))
  es.addEventListener('message', (e) => {
    const { message } = JSON.parse(e.data)
    if (!state.devices.some((d) => d.id === message.deviceId)) loadDevices()
    upsertMessage(message, true)
  })
  es.addEventListener('callbackLog', () => {
    if ($('#tab-callbacks').classList.contains('active')) loadLogs()
  })
  es.onerror = () => renderGateway({ state: 'disconnected', kind: state.gateway?.kind ?? 'serial', port: null, error: 'ホストに接続できません' })
}

// ---------------- boot
;(async () => {
  const status = await api('/status')
  renderGateway(status.gateway)
  renderRadio(status.radio)
  await Promise.all([loadDevices(), loadCallbacks()])
  await loadMessages()
  connectEvents()
  const tab = location.hash.slice(1)
  showTab(['messages', 'devices', 'callbacks', 'settings'].includes(tab) ? tab : 'messages')
})()
