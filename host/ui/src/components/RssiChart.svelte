<script lang="ts">
  import type { LinkStats } from '../lib/api.gen'
  import { t } from '../lib/i18n'
  import { rssiAxis, timeAxis } from '../lib/link'

  let { stats }: { stats: LinkStats } = $props()

  const HEIGHT = 200
  const PAD = { top: 8, right: 28, bottom: 24, left: 40 }
  const TICK_SPACING = 80

  // 点が丸いまま描けるよう、幅を測ってピクセルで描く
  let width = $state(0)
  const plotWidth = $derived(Math.max(0, width - PAD.left - PAD.right))
  const plotHeight = HEIGHT - PAD.top - PAD.bottom

  const yAxis = $derived(rssiAxis(stats.rssi.min, stats.rssi.max))
  const xAxis = $derived(timeAxis(stats.first, stats.last, Math.max(2, Math.floor(plotWidth / TICK_SPACING))))
  const x = (time: number): number => PAD.left + ((time - xAxis.lo) / (xAxis.hi - xAxis.lo)) * plotWidth
  const y = (rssi: number): number => PAD.top + ((yAxis.hi - rssi) / (yAxis.hi - yAxis.lo)) * plotHeight
  // 1 万点でも軽いよう、すべての点を 1 本の path の丸い線端で描く
  const dots = $derived(stats.points.map((p) => `M${x(p.time).toFixed(1)} ${y(p.rssi).toFixed(1)}h0.01`).join(''))
  const label = $derived(
    t('link.chartLabel', { min: stats.rssi.min, avg: stats.rssi.avg.toFixed(1), max: stats.rssi.max }),
  )
</script>

<div class="rssi-chart" bind:clientWidth={width}>
  {#if plotWidth}
    <svg {width} height={HEIGHT} role="img" aria-label={label}>
      {#each yAxis.ticks as v}
        <line class="grid" x1={PAD.left} x2={width - PAD.right} y1={y(v)} y2={y(v)} />
        <text x={PAD.left - 8} y={y(v)} dy="0.32em" text-anchor="end">{v}</text>
      {/each}
      {#each xAxis.ticks as tick}
        <text x={x(tick.time)} y={HEIGHT - 6} text-anchor="middle">{tick.label}</text>
      {/each}
      <line class="avg" x1={PAD.left} x2={width - PAD.right} y1={y(stats.rssi.avg)} y2={y(stats.rssi.avg)} />
      <path class="dots" d={dots} />
    </svg>
  {/if}
</div>
