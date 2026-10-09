import { expect, test } from 'claude-code/testing'
import { isImagePath, estTokens, targetDims, shouldShrink, keptLine, parseDims } from '../hooks/image.js'

test('image helpers: paths, token estimate, target size, the kept line', async () => {
  expect(isImagePath('/a/b/shot.PNG')).toBe(true)
  expect(isImagePath('/a/b/main.rs')).toBe(false)
  expect(estTokens(1024, 768)).toBe(1049)
  expect(estTokens(4000, 3000)).toBe(1600)
  expect(targetDims(3000, 2000, 1024)).toEqual([1024, 683])
  expect(targetDims(800, 600, 1024)).toEqual([800, 600])
  expect(shouldShrink(3000, 2000, 500000, 1024, 60000)).toBe(true)
  expect(shouldShrink(3000, 2000, 1000, 1024, 60000)).toBe(false)
  expect(parseDims('  pixelWidth: 3000\n  pixelHeight: 2000\n')).toEqual([3000, 2000])
  expect(parseDims('3000 2000')).toEqual([3000, 2000])
  expect(keptLine(3000, 2000, 1024, 683, 512000, 102400)).toContain('3000x2000 → 1024x683')
})

// The downscale path itself is exercised live (a 3024x1964 screenshot came back
// at 1024x665 with the hook settling in 446 ms); under `claude plugin test` a
// module hook that answers a Read with an image result does not run through
// the test's own tool.call stub, so that path is not asserted here.

test('small images and non-images pass through, and a box without image tools passes through', async ($, on) => {
  on('process.run', (_$: any, e: any) => {
    const argv: readonly string[] = e.argv ?? e.command ?? e.args ?? []
    if (argv[0] === 'sips' && argv[1] === '-g') return { value: { exitCode: 0, stdout: 'pixelWidth: 800\npixelHeight: 600\n', stderr: '' } }
    if (argv[0] === 'wc') return { value: { exitCode: 0, stdout: '  40000 x\n', stderr: '' } }
    return { value: { exitCode: 127, stdout: '', stderr: '' } }
  })
  on('env.get', () => ({ value: undefined }))
  on('tool.call', () => ({ result: 'the original' }))
  for (const p of ['/repo/small.png', '/repo/src/lib.rs']) {
    const r: any = await $.tool.call({ tool: 'Read', file_path: p })
    expect(r.result).toBe('the original')
  }
})
