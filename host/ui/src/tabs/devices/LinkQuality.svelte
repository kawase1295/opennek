<script lang="ts">
  import type { Auth, LinkStats } from '../../lib/api.gen'
  import { app } from '../../lib/app.svelte'
  import { authBadge } from '../../lib/badges'
  import { fmtTime } from '../../lib/format'
  import { t } from '../../lib/i18n'
  import { percent } from '../../lib/link'
  import EmptyState from '../../components/EmptyState.svelte'
  import PageHeader from '../../components/PageHeader.svelte'
  import RssiChart from '../../components/RssiChart.svelte'

  let { id, stats }: { id: string; stats: LinkStats | null } = $props()

  const name = $derived(app.deviceName(id))
  const AUTHS: Auth[] = ['ok', 'public', 'unverified', 'bad']
</script>

{#snippet bar(label: string, count: number, total: number, tone: string, share?: string)}
  <span>{label}</span>
  <span class="track"><i class={tone} style:width="{(count / total) * 100}%"></i></span>
  {#if share !== undefined}<span class="num">{share}</span>{/if}
  <span class="num muted">{count}</span>
{/snippet}

<PageHeader title={t('link.title', { device: name ? `${id} (${name})` : id })} intro={t('link.intro')}>
  {#snippet actions()}
    <button class="ghost" onclick={() => (app.deviceLink = null)}>← {t('link.back')}</button>
  {/snippet}
</PageHeader>
{#if stats}
  {@const expected = stats.messages + stats.missed}
  <div class="link-tiles">
    <div class="card tile">
      <span>{t('link.messages')}</span>
      <strong>{stats.messages}</strong>
      <small>{fmtTime(stats.first)} – {fmtTime(stats.last)}</small>
    </div>
    <div class="card tile">
      <span>{t('link.rssiAvg')}</span>
      <strong>{stats.rssi.avg.toFixed(1)} dBm</strong>
      <small>{t('link.rssiRange', { min: stats.rssi.min, max: stats.rssi.max })}</small>
    </div>
    <div class="card tile">
      <span>{t('link.missed')}</span>
      <strong>{stats.missed}</strong>
      <small>{t('link.missedShare', { share: percent(stats.missed, expected), expected })}</small>
    </div>
  </div>
  <div class="card link-chart">
    <div class="head">
      <h3>{t('link.chart')}</h3>
      <span class="legend"><i></i>{t('link.average')} {stats.rssi.avg.toFixed(1)}</span>
    </div>
    <RssiChart {stats} />
  </div>
  <div class="cards">
    <div class="card">
      <h3>{t('link.frames')}</h3>
      <div class="link-bars">
        {#each stats.frames.map((count, i) => ({ label: `${i + 1}/3`, count })).reverse() as row (row.label)}
          {@render bar(row.label, row.count, stats.messages, 'accent', percent(row.count, stats.messages))}
        {/each}
      </div>
    </div>
    <div class="card">
      <h3>{t('link.auth')}</h3>
      <div class="link-bars counts">
        {#each AUTHS as auth (auth)}
          {@const badge = authBadge(auth)}
          {@render bar(badge.label, stats.auth[auth], stats.messages, badge.tone)}
        {/each}
      </div>
    </div>
  </div>
{:else}
  <EmptyState boxed title={t('link.empty')} />
{/if}
