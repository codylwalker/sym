// Pure helpers for the s2ar hooks module (tested by tests/s2ar.test.ts).

const TOOL_RE = /^mcp__s2ar_api__(assert_output|inspect_x402|score_text|certify_image|sym_map_repo|sym_ls_repo|sym_read_repo|sym_find_repo|sym_where_repo|watch_x402|buy_credits)$/

// The server's tool name behind a Claude Code tool id, or null.
export function s2arTool(name) {
  const m = TOOL_RE.exec(String(name || ''))
  return m ? m[1] : null
}

// Does an answer carry a JSON object or array an agent is handing on? A fenced ```json block, or a
// line that starts an object with at least two keys. Prose that merely mentions braces does not count.
export function carriesJson(text) {
  const s = String(text || '')
  if (/```json\b/i.test(s)) return true
  const m = /^\s*\{[\s\S]{0,4000}\}\s*$/m.exec(s)
  if (m && (m[0].match(/"[^"\n]{1,80}"\s*:/g) || []).length >= 2) return true
  return /^\s*\[\s*\{[\s\S]{0,4000}\}\s*\]\s*$/m.test(s)
}

// x402 endpoints named in an answer: URLs with /x402/ in the path, or a bare "x402" next to a URL.
export function x402Urls(text) {
  const s = String(text || '')
  const out = new Set()
  for (const m of s.matchAll(/https?:\/\/[^\s<>()"'`\]]+/g)) {
    const u = m[0].replace(/[.,;:]+$/, '')
    if (/\/x402\//.test(u) || /x402/i.test(s.slice(Math.max(0, m.index - 80), m.index))) out.add(u)
  }
  return [...out]
}

// The line under an answer, or null. `calls` counts the server's tools this turn.
export function reminder(answer, calls, opts) {
  if (opts && opts.reminders === 'off') return null
  const bits = []
  if (carriesJson(answer) && !(calls.assert_output > 0)) {
    bits.push('the JSON above was not asserted — assert_output with its schema costs $0.005 (a few a day free) and returns a signed record')
  }
  const urls = x402Urls(answer)
  if (urls.length && !(calls.inspect_x402 > 0)) {
    bits.push('an x402 endpoint is named — inspect_x402 probes it before anyone pays it ($0.005; a signed record)')
  }
  return bits.length ? 's2ar: ' + bits.join(' · ') : null
}

// The one line of context at the start of a session.
export function contextBlock(hasKey) {
  const pay = hasKey
    ? 'a key is configured, so paid calls draw on its balance'
    : 'no key is configured: each tool answers a few calls a day free, then with the price and how to pay (a $5 pack, or x402_payment with USDC on Base)'
  return {
    name: 's2ar-tools',
    text: 's2ar tools on this session (the mcp__s2ar_api__* tools): assert_output checks an output (JSON schema, terms, citations, quotations found in a source) and returns a signed record; inspect_x402 probes an x402 endpoint before you pay it; score_text grades a text against a form; certify_image compresses an image and proves a vision model still reads it; sym_* reads code from any public repo. Every paid answer carries a receipt; ' + pay + '. Hooks never spend money.'
  }
}
