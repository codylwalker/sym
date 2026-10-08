import { expect, test } from 'claude-code/testing'

// A fake `sym` binary: answers ls --json for a big and a small file, ls --est
// with a skeleton, and anything else with an empty success.
function fakeSym(argv: readonly string[]) {
  const [cmd, ...rest] = argv
  if (cmd !== 'sym') return null
  const path = rest[rest.length - 1]
  if (rest[0] === 'ls' && rest[1] === '--json') {
    const lines = path.includes('big') ? 900 : 40
    return { exitCode: 0, stdout: JSON.stringify({ path, lines, symbols: [{ path: 'alpha', name: 'alpha', kind: 'fn', start_line: 1, end_line: 3, signature: 'pub fn alpha()', depth: 0 }] }), stderr: '' }
  }
  if (rest[0] === 'ls' && rest[1] === '--est') return { exitCode: 0, stdout: path + ' — 900 lines, 1 symbols\n    1-3     pub fn alpha()\n~40 tokens\n', stderr: '' }
  if (rest[0] === 'read') return { exitCode: 0, stdout: 'fn alpha (' + rest[1] + ', lines 1-3)\n    1\tpub fn alpha() {}\n', stderr: '' }
  if (rest[0] === 'map') return { exitCode: 0, stdout: 'src/lib.rs:\n  pub fn alpha()\n', stderr: '' }
  return { exitCode: 0, stdout: '', stderr: '' }
}

function stubWorld(on: any) {
  on('process.run', (_$: any, e: any) => {
    const argv: readonly string[] = e.argv ?? e.command ?? e.args ?? []
    const r = argv[0] === 'git' ? { exitCode: 0, stdout: 'abc123\n', stderr: '' } : fakeSym(argv)
    return { value: r ?? { exitCode: 127, stdout: '', stderr: 'nope' } }
  })
  on('fs.stat', () => ({ value: { kind: 'file', size: 36000, mtimeMs: 0, isLink: false } }))
  on('fs.exists', () => ({ value: false }))
  on('env.get', () => ({ value: undefined }))
  on('session.cwd', () => ({ value: '/repo' }))
}

test('a whole-file Read of a big source file comes back as the skeleton', async ($, on) => {
  stubWorld(on)
  let ran = false
  on('tool.call', () => { ran = true; return { result: 'the whole file' } })
  const r: any = await $.tool.call({ tool: 'Read', file_path: '/repo/src/big.rs' })
  expect(ran).toBe(false)
  expect(r.result.type).toBe('text')
  expect(r.result.file.totalLines).toBe(900)
  expect(r.result.file.content).toContain('pub fn alpha()')
  expect(r.result.file.content).toContain('[sym] Whole file not loaded (900 lines')
})

test('ranged, small and non-source Reads pass through untouched', async ($, on) => {
  stubWorld(on)
  on('tool.call', () => ({ result: 'the whole file' }))
  for (const call of [
    { tool: 'Read', file_path: '/repo/src/big.rs', offset: 10, limit: 40 },
    { tool: 'Read', file_path: '/repo/src/small.rs' },
    { tool: 'Read', file_path: '/repo/README.md' },
  ]) {
    const r: any = await $.tool.call(call as any)
    expect(r.result).toBe('the whole file')
  }
})

test('the sym tools shell to the binary and the stats command counts', async ($, on) => {
  stubWorld(on)
  const r: any = await $.tool.call({ tool: 'mcp__sym__read', file: '/repo/src/big.rs', symbol: 'alpha' } as any)
  expect(r.result).toContain('fn alpha (')
  const m: any = await $.tool.call({ tool: 'mcp__sym__map', dir: '/repo' } as any)
  expect(m.result).toContain('src/lib.rs:')
  const answer: any = await $.command.run({ command: 'sym-stats', args: '' })
  expect(answer.text).toContain('sym:')
})

test('a failing sym binary never blocks a Read', async ($, on) => {
  on('process.run', () => ({ value: { exitCode: 127, stdout: '', stderr: 'sym: not found' } }))
  on('fs.exists', () => ({ value: false }))
  on('env.get', () => ({ value: undefined }))
  on('session.cwd', () => ({ value: '/repo' }))
  on('tool.call', () => ({ result: 'the whole file' }))
  const r: any = await $.tool.call({ tool: 'Read', file_path: '/repo/src/big.rs' })
  expect(r.result).toBe('the whole file')
})

test('SYM_INDEX_URL and SYM_INDEX_DIR wire the where tool without plugin config', async ($, on) => {
  const seen: string[][] = []
  on('process.run', (_$: any, e: any) => {
    const argv: readonly string[] = e.argv ?? e.command ?? e.args ?? []
    seen.push([...argv])
    return { value: argv[0] === 'sym' ? { exitCode: 0, stdout: '0.9  src/a.rs:1-3  fn  alpha  pub fn alpha()\n', stderr: '' } : { exitCode: 0, stdout: 'abc123\n', stderr: '' } }
  })
  on('fs.stat', () => ({ value: { kind: 'file', size: 100, mtimeMs: 0, isLink: false } }))
  on('fs.exists', () => ({ value: true }))
  on('env.get', (_$: any, e: any) => ({ value: ({ SYM_INDEX_URL: 'http://embed:8440', SYM_INDEX_DIR: '/idx' } as any)[e.name ?? e.key ?? ''] }))
  on('session.cwd', () => ({ value: '/repo' }))
  const r: any = await $.tool.call({ tool: 'mcp__sym__where', query: 'binary detection' } as any)
  expect(r.result).toContain('alpha')
  const call = seen.find((a) => a[0] === 'sym' && a[1] === 'where')!
  expect(call.join(' ')).toContain('--index /idx')
  expect(call.join(' ')).toContain('--embed-url http://embed:8440')
})
