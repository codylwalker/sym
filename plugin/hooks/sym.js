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
//   turn.complete   prints what was kept out of context this turn
//
// Every shell call goes through the `sym` binary (cargo install sym-cli).
// When it is missing, every hook passes the event on unchanged.

const SOURCE_EXT = /\.(rs|lua|py|pyi|ts|tsx|mts|cts|js|jsx|mjs|cjs|go|c|h|cpp|cc|cxx|hpp|hh|hxx|java|rb)$/i

// Per-turn tallies (reset in turn.complete). Plain module state is fine: a
// hooks module lives for the session.
let turn = { skeletons: 0, bytesKept: 0, symbolReads: 0 }
let total = { skeletons: 0, bytesKept: 0, symbolReads: 0, turns: 0 }
let repoMap = ''
let indexDir = ''
let indexUrl = ''
let indexModel = ''
let indexDirOverride = ''
let envResolved = false

// The plugin config sets the index URL and model for an installed plugin; the
// environment (SYM_INDEX_URL, SYM_INDEX_MODEL, SYM_INDEX_DIR) covers
// --plugin-dir runs and the bench. Resolved once, on first use.
async function resolveEnv($) {
  if (envResolved) return
  envResolved = true
  indexUrl = indexUrl || (await $.env.get('SYM_INDEX_URL')) || ''
  indexModel = indexModel || (await $.env.get('SYM_INDEX_MODEL')) || ''
  indexDirOverride = (await $.env.get('SYM_INDEX_DIR')) || ''
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
  const mapBudget = Number((options && options.map_budget) || 600)
  // The plugin config sets these for an installed plugin; the environment
  // (SYM_INDEX_URL, SYM_INDEX_MODEL, SYM_INDEX_DIR) covers --plugin-dir runs
  // and the bench, and is resolved once the session starts.
  indexUrl = (options && options.index_url) || ''
  indexModel = (options && options.index_model) || ''

  on('session.start', async ($, e, next) => {
    const cwd = await $.session.cwd()
    await resolveEnv($)
    const head = await gitHead($, cwd)
    const key = 'sym.map.' + cwd + '@' + head
    // The repo map, cached per HEAD so prompt.context stays cache-stable.
    const cached = await $.store.get(key)
    if (typeof cached === 'string') {
      repoMap = cached
    } else {
      const r = await symRun($, ['map', cwd, '--budget', String(mapBudget)], cwd)
      repoMap = r.exitCode === 0 ? r.stdout : ''
      if (repoMap) await $.store.set(key, repoMap)
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
    return { text: 'sym: ' + total.skeletons + ' skeleton answers · ~' + tokens(total.bytesKept) + ' tokens kept out of context · ' + total.symbolReads + ' symbol reads · ' + total.turns + ' turns' }
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

  on('turn.complete', async ($, e, next) => {
    const t = { ...turn }
    total.skeletons += t.skeletons
    total.bytesKept += t.bytesKept
    total.symbolReads += t.symbolReads
    total.turns += 1
    turn = { skeletons: 0, bytesKept: 0, symbolReads: 0 }
    const r = await next(e)
    if (t.skeletons === 0 && t.symbolReads === 0) return r
    const line = 'sym: ' + t.skeletons + ' skeleton' + (t.skeletons === 1 ? '' : 's') + ' · ~' + tokens(t.bytesKept) + ' tokens kept out · ' + t.symbolReads + ' symbol read' + (t.symbolReads === 1 ? '' : 's')
    return { ...r, text: ((r && r.text) ? r.text + '\n' : '') + line }
  })
}
