import { s2arTool, reminder, contextBlock } from './scan.js'

// s2ar — the hosted tools as MCP tools; this module only adds context and reminders.
//
//   prompt.context   one block naming the tools and how paying works (static; no network)
//   tool.call        counts the server's tools per turn (never alters a call)
//   turn.complete    a line under the answer when JSON went out unasserted, or an x402
//                    endpoint was named and not inspected — a reminder, never a call

let turn = {}
let total = { turns: 0, assert_output: 0, inspect_x402: 0 }
let opts = { reminders: 'on' }
let hasKey = false

export function register(on, options) {
  opts = { reminders: (options && options.reminders) || 'on' }
  hasKey = Boolean(options && options.api_key)

  on('prompt.context', async ($, e, next) => {
    const r = await next(e)
    return { ...r, blocks: [...((r && r.blocks) || []), contextBlock(hasKey)] }
  })

  on('tool.call', { tool: /^mcp__s2ar_api__/ }, async ($, e, next) => {
    try {
      const name = s2arTool(e.tool)
      if (name) {
        turn[name] = (turn[name] || 0) + 1
        if (name in total) total[name] += 1
      }
    } catch {
      // counting is cosmetic; the call always goes through
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const calls = { ...turn }
    turn = {}
    total.turns += 1
    const r = await next(e)
    const line = reminder(e && e.answer, calls, opts)
    if (!line) return r
    return { ...r, text: ((r && r.text) ? r.text + '\n' : '') + line }
  })
}
