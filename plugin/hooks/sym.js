import { filesOf, withSummaries, summaryPrompt, cleanSummary } from './summaries.js'
import { usdDelta, turnLine, meterSummary } from './meter.js'
// sym — the mod. Read the function, not the file.
//
// What it does, in the order a session meets it:
//   session.start   registers the tools (map, ls, read, find, where when an
//                   index exists) and the /sym-stats and /sym-index commands;
//                   caches the repo map per HEAD; starts the indexer when the
//                   repo changed and an embedding endpoint is configured
//   prompt.context  sends the cached repo map with the first message
//   tool.describe   tells Claude that big source Reads come back as skeletons
//   tool.call(Read) answers a whole-file Read of a big source file with its
//                   skeleton (read_mode=skeleton), or appends a note (hint),
//                   or does nothing (off); ranged Reads always pass through
//   session.measure samples the engine's cost ledger (the meter)
//   turn.complete   prints what was kept out and what the turn cost
//
// Every shell call goes through the `sym` binary (cargo install starlab-sym).
// When it is missing, every hook passes the event on unchanged.

const SOURCE_EXT = /\.(rs|lua|py|pyi|ts|tsx|mts|cts|js|jsx|mjs|cjs|go|c|h|cpp|cc|cxx|hpp|hh|hxx|java|rb)$/i

// Per-turn tallies (reset in turn.complete). Plain module state is fine: a
// hooks module lives for the session.
let turn = { skeletons: 0, bytesKept: 0, symbolReads: 0 }
let total = { skeletons: 0, bytesKept: 0, symbolReads: 0, turns: 0 }
// The meter: the engine's own cost ledger, sampled at every session.measure.
let meter = { lastUsd: 0, usd: 0, turnUsd: 0, series: [], context: null, sessionId: '' }
let repoMap = ''
let indexDir = ''
let indexUrl = ''
let indexModel = ''
let indexDirOverride = ''
let summariesMode = ''
let mapBudget = 600
let envResolved = false

// The plugin config sets the index URL and model for an installed plugin; the
// environment (SYM_INDEX_URL, SYM_INDEX_MODEL, SYM_INDEX_DIR) covers
// --plugin-dir runs and the bench. Resolved once, on first use.
async function resolveEnv($) {
  if (envResolved) return
  envResolved = true
  // The environment wins over the plugin config: a manifest default (such
  // as summaries "off") must not shadow a switch set for one session.
  indexUrl = (await $.env.get('SYM_INDEX_URL')) || indexUrl || ''
  indexModel = (await $.env.get('SYM_INDEX_MODEL')) || indexModel || ''
  indexDirOverride = (await $.env.get('SYM_INDEX_DIR')) || ''
  summariesMode = (await $.env.get('SYM_SUMMARIES')) || summariesMode || 'off'
  const mb = await $.env.get('SYM_MAP_BUDGET')
  if (mb !== undefined && mb !== null && mb !== '') mapBudget = Number(mb)
}

function tokens(bytes) {
  return Math.ceil(bytes / 4)
}

async function symRun($, args, cwd) {
  try {
    const r = await $.process.run(['sym', ...args], cwd ? { cwd } : undefined)
    return r
  } catch {
    return { exitCode: 127, stdout: '', stderr: 'sym not installed' }
  }
}

// Is there an index at `dir`? $.fs answers for the plugin's own dirs; an
// override dir anywhere else is asked through the shell.
async function indexReady($, dir) {
  try {
    return await $.fs.exists(dir + '/meta.json')
  } catch {
    const r = await $.process.run({ argv: ['ls', dir + '/meta.json'] })
    return r.exitCode === 0
  }
}

// One short completion per file of the map, eight at a time, stored as
// { file: summary }. Each answer is the plan's own spend (haiku), so it is
// opt-in and capped at forty files.
async function summarize($, mapText, sumKey) {
  const files = filesOf(mapText).slice(0, 40)
  const out = {}
  for (let i = 0; i < files.length; i += 8) {
    const batch = files.slice(i, i + 8)
    const answers = await Promise.all(batch.map(async (f) => {
      try {
        const r = await $.model.complete({ model: 'haiku', prompt: summaryPrompt(f.file, f.block) })
        if (!(r && r.isAnswered)) await diag($, 'summary of ' + f.file + ' not answered: ' + (r && r.reason ? JSON.stringify(r.reason).slice(0, 160) : JSON.stringify(r).slice(0, 160)))
        return r && r.isAnswered ? cleanSummary(r.text) : ''
      } catch (err) {
        await diag($, 'summary of ' + f.file + ' threw: ' + (err && err.message ? err.message : String(err)))
        return ''
      }
    }))
    batch.forEach((f, j) => { if (answers[j]) out[f.file] = answers[j] })
  }
  if (Object.keys(out).length) await $.store.set(sumKey, out)
  await diag($, 'summaries ' + Object.keys(out).length + '/' + files.length + ' files')
  return out
}

// Diagnostic lines kept in the store (capped), readable through the `diag`
// tool, since neither $.ui.log nor $.fs reach a file we can read from outside.
async function diag($, line) {
  try {
    const prev = await $.store.get('sym.diag')
    const lines = Array.isArray(prev) ? prev : []
    lines.push(new Date().toISOString() + ' ' + line)
    await $.store.set('sym.diag', lines.slice(-60))
  } catch {
    // diagnostics never matter to the session
  }
}

async function gitHead($, cwd) {
  try {
    const r = await $.process.run(['git', 'rev-parse', 'HEAD'], { cwd })
    return r.exitCode === 0 ? r.stdout.trim() : 'nogit'
  } catch {
    return 'nogit'
  }
}

export function register(on, options) {
  const readMode = (options && options.read_mode) || 'skeleton'
  const minLines = Number((options && options.min_lines) || 200)
  mapBudget = Number((options && options.map_budget) || 600)
  // The plugin config sets these for an installed plugin; the environment
  // (SYM_INDEX_URL, SYM_INDEX_MODEL, SYM_INDEX_DIR) covers --plugin-dir runs
  // and the bench, and is resolved once the session starts.
  indexUrl = (options && options.index_url) || ''
  indexModel = (options && options.index_model) || ''
  summariesMode = (options && options.summaries) || ''

  on('session.start', async ($, e, next) => {
    const cwd = await $.session.cwd()
    await resolveEnv($)
    const head = await gitHead($, cwd)
    const key = 'sym.map.' + cwd + '@' + head
    // The repo map, cached per HEAD so prompt.context stays cache-stable.
    const cached = await $.store.get(key)
    if (!(mapBudget > 0)) {
      repoMap = ''   // SYM_MAP_BUDGET=0 / map_budget 0: no map with the first message
    } else if (typeof cached === 'string') {
      repoMap = cached
    } else {
      const r = await symRun($, ['map', cwd, '--budget', String(mapBudget)], cwd)
      repoMap = r.exitCode === 0 ? r.stdout : ''
      if (repoMap) await $.store.set(key, repoMap)
    }
    // File summaries (opt-in): one haiku line per file, cached per HEAD, and
    // computed AFTER this hook returns so the session never waits on them;
    // the first session gets the plain map, later ones the annotated one.
    if ((summariesMode === 'haiku' || summariesMode === 'haiku-wait') && repoMap) {
      const sumKey = key + ':summaries'
      const have = await $.store.get(sumKey)
      await diag($, 'summaries: mode=' + summariesMode + ' have=' + (have && typeof have === 'object' ? Object.keys(have).length + ' files' : String(have)))
      if (have && typeof have === 'object' && Object.keys(have).length > 0) {
        repoMap = withSummaries(repoMap, have)
      } else if (summariesMode === 'haiku-wait') {
        // Wait for them (the model's time is not charged to the hook's budget):
        // the first session already gets the annotated map.
        try {
          const t0 = Date.now()
          repoMap = withSummaries(repoMap, await summarize($, repoMap, sumKey))
          await diag($, 'summaries took ' + (Date.now() - t0) + ' ms')
        } catch (err) {
          $.ui.log('sym: summaries failed: ' + (err && err.message ? err.message : String(err)))
        }
      } else {
        summarize($, repoMap, sumKey).catch((err) => $.ui.log('sym: summaries failed: ' + (err && err.message ? err.message : String(err))))
      }
    }
    // Tools for Claude. Each handler shells to sym and returns its text.
    await $.tool.register({
      name: 'map',
      isDeferred: false,
      description: 'Repo map fitted to a token budget: per-file top-level signatures, files ranked by PageRank over the import graph. Orientation before reading anything.',
      inputSchema: { type: 'object', properties: { dir: { type: 'string' }, budget: { type: 'integer' } }, required: [] },
    })
    await $.tool.register({
      name: 'ls',
      isDeferred: false,
      description: 'Skeleton of one source file: every symbol with its line range and signature (rs/lua/py/ts/js/go/c/cpp/java/rb). Use before any Read of a big file.',
      inputSchema: { type: 'object', properties: { file: { type: 'string' } }, required: ['file'] },
    })
    await $.tool.register({
      name: 'read',
      isDeferred: false,
      description: 'One symbol\'s source, line-numbered, with its doc block. `symbol` is the leaf name or the qualified path from ls (Widget::new, Runner.helper, Server.Serve); `impl Trait for Type`, `Type as Trait`, `Type::method` and short trait paths are accepted too. For "where is X defined", one read of X answers it; prefer Grep when you need mentions, not the definition.',
      inputSchema: { type: 'object', properties: { file: { type: 'string' }, symbol: { type: 'string' } }, required: ['file', 'symbol'] },
    })
    await $.tool.register({
      name: 'find',
      isDeferred: false,
      description: 'Definitions by name across the tree: every symbol whose leaf name or qualified path equals `name` (or starts with it when prefix is true). Grep finds mentions; this finds the definition.',
      inputSchema: { type: 'object', properties: { name: { type: 'string' }, dir: { type: 'string' }, prefix: { type: 'boolean' } }, required: ['name'] },
    })
    // The semantic tool only when an index exists for this checkout.
    // The semantic index: never let it cost the session. $.fs is scoped to
    // the plugin's own directories, so an index dir elsewhere (SYM_INDEX_DIR)
    // is probed through the shell instead; any failure here just means no
    // `where` tool this session.
    indexDir = ''
    try {
      const data = indexDirOverride || (await $.env.get('CLAUDE_PLUGIN_DATA'))
      if (indexUrl && data) {
        indexDir = indexDirOverride || (data + '/index-' + head.slice(0, 12))
        if (!(await indexReady($, indexDir))) {
          $.ui.status('sym: indexing ' + cwd + ' …')
          const args = ['index', cwd, '--out', indexDir, '--embed-url', indexUrl]
          if (indexModel) args.push('--embed-model', indexModel)
          symRun($, args, cwd).then((r) => {
            $.ui.status(r.exitCode === 0 ? 'sym: index ready' : 'sym: index failed')
          })
        }
        await $.tool.register({
          name: 'where',
          description: 'Code by meaning: the symbols whose text best matches a natural-language query, from the semantic index of this checkout. Use when you do not know the name of what you are looking for.',
          inputSchema: { type: 'object', properties: { query: { type: 'string' }, k: { type: 'integer' } }, required: ['query'] },
        })
      }
    } catch (err) {
      indexDir = ''
      $.ui.log('sym: semantic index unavailable this session: ' + (err && err.message ? err.message : String(err)))
    }
    await $.tool.register({
      name: 'diag',
      isDeferred: false,
      description: 'sym plugin diagnostics (the last lines the mod logged); for debugging the plugin, not the code.',
      inputSchema: { type: 'object', properties: {} },
    })
    try {
      await $.command.register({ name: 'sym-stats', description: 'What sym kept out of context this session' })
      await $.command.register({ name: 'sym-index', description: 'Rebuild the semantic index for this checkout now' })
    } catch {
      // a taken name only loses the command, never the session
    }
    return next(e)
  })

  on('prompt.context', async ($, e, next) => {
    const r = await next(e)
    if (!repoMap) return r
    const block = { name: 'sym-repo-map', text: 'Repository map (sym, top-level signatures by import rank; use the ls/read/find tools before reading whole files):\n' + repoMap }
    return { ...r, blocks: [...(r.blocks || []), block] }
  })

  on('tool.describe', { tool: 'Read' }, async ($, e, next) => {
    if (readMode === 'off') return next(e)
    const r = await next(e)
    const add = ' Source files over ' + minLines + ' lines come back as a symbol skeleton (sym); read a range with offset/limit, or one symbol with the sym read tool.'
    return { ...r, description: (r.description || '') + add }
  })

  on('tool.call', { tool: 'Read' }, async ($, e, next) => {
    const path = e.file_path || ''
    if (readMode === 'off' || !SOURCE_EXT.test(path) || e.offset !== undefined || e.limit !== undefined) {
      return next(e)
    }
    const ls = await symRun($, ['ls', '--json', path])
    if (ls.exitCode !== 0) return next(e)
    let info
    try {
      info = JSON.parse(ls.stdout)
    } catch {
      return next(e)
    }
    if (!info || info.lines <= minLines || !info.symbols || info.symbols.length === 0) return next(e)
    const text = await symRun($, ['ls', '--est', path])
    const skeleton = text.exitCode === 0 ? text.stdout : ls.stdout
    let bytes = 0
    try {
      bytes = Number((await $.fs.stat(path)).size || 0)
    } catch {
      bytes = info.lines * 40
    }
    const note = '\n[sym] Whole file not loaded (' + info.lines + ' lines, ~' + tokens(bytes) + ' tokens). Read with offset/limit for a range, or the sym read tool for one symbol.\n'
    if (readMode === 'hint') {
      const r = await next(e)
      const f = r && r.result && r.result.file
      if (f && typeof f.content === 'string') {
        return { ...r, result: { ...r.result, file: { ...f, content: f.content + note + skeleton } } }
      }
      return r
    }
    turn.skeletons += 1
    turn.bytesKept += Math.max(0, bytes - skeleton.length)
    // A synthetic Read result must match Read's own output shape.
    const content = skeleton + note
    return { result: { type: 'text', file: { filePath: path, content, numLines: content.split('\n').length, startLine: 1, totalLines: info.lines } } }
  }).catch(async ($, e, next) => next(e))

  on('tool.call', { tool: 'mcp__sym__diag' }, async ($) => {
    const lines = await $.store.get('sym.diag')
    return { result: Array.isArray(lines) && lines.length ? lines.join('\n') : 'sym: no diagnostics recorded' }
  })

  on('tool.call', { tool: /^mcp__sym__(map|ls|read|find|where)$/ }, async ($, e) => {
    const cwd = await $.session.cwd()
    const name = e.tool.replace('mcp__sym__', '')
    let args
    if (name === 'map') args = ['map', e.dir || cwd, '--budget', String(e.budget || mapBudget)]
    else if (name === 'ls') args = ['ls', e.file]
    else if (name === 'read') args = ['read', e.file, e.symbol]
    else if (name === 'find') args = ['find', e.name, e.dir || cwd].concat(e.prefix ? ['--prefix'] : [])
    else if (name === 'where') {
      await resolveEnv($)
      if (!indexDir && indexDirOverride) indexDir = indexDirOverride
      args = ['where', e.query, '--index', indexDir, '--k', String(e.k || 8)].concat(indexUrl ? ['--embed-url', indexUrl] : [])
    }
    else return { result: 'unknown sym tool' }
    const r = await symRun($, args, cwd)
    if (name === 'read' && r.exitCode === 0) turn.symbolReads += 1
    return { result: r.exitCode === 0 ? r.stdout : ('sym ' + name + ' failed: ' + (r.stderr || r.stdout)).trim() }
  })

  on('command.run', { command: 'sym-stats' }, async () => {
    const kept = 'sym: ' + total.skeletons + ' skeleton answers · ~' + tokens(total.bytesKept) + ' tokens kept out of context · ' + total.symbolReads + ' symbol reads · ' + total.turns + ' turns'
    return { text: kept + '\n' + meterSummary(meter.series, tokens(total.bytesKept)) }
  })

  on('command.run', { command: 'sym-index' }, async ($) => {
    await resolveEnv($)
    if (!indexUrl) return { text: 'sym: set index_url in the plugin config to enable the semantic index' }
    const cwd = await $.session.cwd()
    const data = await $.env.get('CLAUDE_PLUGIN_DATA')
    const head = await gitHead($, cwd)
    indexDir = indexDirOverride || (data + '/index-' + head.slice(0, 12))
    const args = ['index', cwd, '--out', indexDir, '--embed-url', indexUrl]
    if (indexModel) args.push('--embed-model', indexModel)
    const r = await symRun($, args, cwd)
    return { text: r.exitCode === 0 ? 'sym: index rebuilt at ' + indexDir : 'sym: index failed: ' + r.stderr }
  })

  // The meter samples the engine's ledger after every turn; the series is
  // stored per session so the bench can check it against the session's own
  // reported cost. Watch-only: next(e) is passed through unchanged.
  on('session.measure', async ($, e, next) => {
    try {
      const usd = e && e.cost && Number.isFinite(e.cost.usd) ? e.cost.usd : meter.lastUsd
      const delta = usdDelta(meter.lastUsd, usd)
      meter.lastUsd = usd
      meter.usd = usd
      meter.turnUsd += delta
      meter.context = e && e.context ? { percent: e.context.percent, window: e.context.window, tokens: e.context.tokens } : meter.context
      meter.series.push({ usd, delta, context: meter.context, at: Date.now() })
      if (!meter.sessionId) meter.sessionId = String(await $.session.id())
      if (meter.series.length % 1 === 0) await $.store.set('sym.meter.' + meter.sessionId, { usd, turns: meter.series.length, series: meter.series.slice(-200) })
    } catch {
      // the meter never costs the session
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const t = { ...turn }
    total.skeletons += t.skeletons
    total.bytesKept += t.bytesKept
    total.symbolReads += t.symbolReads
    total.turns += 1
    turn = { skeletons: 0, bytesKept: 0, symbolReads: 0 }
    const spent = meter.turnUsd
    meter.turnUsd = 0
    const r = await next(e)
    const bits = []
    if (t.skeletons > 0 || t.symbolReads > 0) {
      bits.push(t.skeletons + ' skeleton' + (t.skeletons === 1 ? '' : 's') + ' · ~' + tokens(t.bytesKept) + ' tokens kept out · ' + t.symbolReads + ' symbol read' + (t.symbolReads === 1 ? '' : 's'))
    }
    if (meter.series.length) bits.push(turnLine(spent, meter.usd, meter.context))
    if (!bits.length) return r
    const line = 'sym: ' + bits.join(' · ')
    return { ...r, text: ((r && r.text) ? r.text + '\n' : '') + line }
  })
}
