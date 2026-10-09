// The meter: what this session spent, per turn, from the engine's own ledger
// (session.measure carries `cost.usd` as /cost totals it, and the live context
// window). Nothing is computed from a price table and nothing leaves the
// machine; the series is kept in the plugin store so the bench can check the
// meter against the session's reported cost.

export function usdDelta(prevUsd, usd) {
  const a = Number(prevUsd) || 0
  const b = Number(usd)
  if (!Number.isFinite(b)) return 0
  return Math.max(0, b - a)
}

export function fmtUsd(usd) {
  const v = Number(usd) || 0
  if (v === 0) return '$0'
  if (v < 0.01) return '$' + v.toFixed(4)
  if (v < 1) return '$' + v.toFixed(3)
  return '$' + v.toFixed(2)
}

export function fmtPercent(p) {
  return Number.isFinite(p) ? Math.round(p) + '%' : '?'
}

// One line for the turn: cost of the turn, the session total, the context.
export function turnLine(delta, totalUsd, context) {
  const bits = [fmtUsd(delta) + ' this turn', fmtUsd(totalUsd) + ' so far']
  if (context && Number.isFinite(context.percent)) bits.push('context ' + fmtPercent(context.percent))
  return bits.join(' · ')
}

// The session summary for /sym-stats.
export function meterSummary(series, keptTokens) {
  const total = series.length ? series[series.length - 1].usd : 0
  const turns = series.length
  const max = series.reduce((m, s) => Math.max(m, s.delta || 0), 0)
  const last = series.length ? series[series.length - 1] : null
  const parts = ['meter: ' + fmtUsd(total) + ' over ' + turns + ' measured turn' + (turns === 1 ? '' : 's'),
    'dearest turn ' + fmtUsd(max)]
  if (last && last.context && Number.isFinite(last.context.percent)) parts.push('context ' + fmtPercent(last.context.percent) + ' of ' + (last.context.window || '?'))
  if (keptTokens) parts.push('~' + keptTokens + ' tokens kept out by sym')
  return parts.join(' · ')
}
