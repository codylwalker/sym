import { isImagePath, estTokens, targetDims, shouldShrink, keptLine, parseDims } from './image.js'

// lens — the screenshot, not the pixels.
//
//   tool.call(Read)  a whole-image Read over the longest edge comes back
//                    downscaled: the engine's own image result shape, a JPEG
//                    at the size a model actually reads, plus one line
//                    saying what was kept out. The file on disk is untouched.
//   tool.describe    tells the model so it plans for it.
//   turn.complete    a line under the answer: images shrunk, tokens kept out.
//
// Resizing uses the box's own tools (`sips` on macOS, ImageMagick's `magick`
// or `convert` elsewhere); when none is present the Read passes through.

let turn = { images: 0, tokensKept: 0 }
let total = { images: 0, tokensKept: 0, turns: 0 }
let maxEdge = 1024
let quality = 85
let minBytes = 60000

async function run($, argv) {
  try {
    const r = await $.process.run({ argv })
    return r && r.exitCode === 0 ? (r.stdout || '') : null
  } catch {
    return null
  }
}

// [w, h, bytes] of a file, through sips (macOS) or ImageMagick; null when unknown.
async function probe($, path) {
  let dims = parseDims(await run($, ['sips', '-g', 'pixelWidth', '-g', 'pixelHeight', path]))
  if (!dims) dims = parseDims(await run($, ['magick', 'identify', '-format', '%w %h', path]))
  if (!dims) dims = parseDims(await run($, ['identify', '-format', '%w %h', path]))
  if (!dims) return null
  const st = await run($, ['wc', '-c', path])
  const bytes = st ? Number((st.trim().split(/\s+/)[0]) || 0) : 0
  return [dims[0], dims[1], bytes]
}

async function shrink($, path, tw, th, out) {
  // sips: -Z fits the longest edge; format jpeg with quality.
  let ok = await run($, ['sips', '-Z', String(Math.max(tw, th)), '-s', 'format', 'jpeg', '-s', 'formatOptions', String(quality), path, '--out', out])
  if (ok === null) ok = await run($, ['magick', path, '-resize', tw + 'x' + th, '-quality', String(quality), '-background', 'white', '-flatten', out])
  if (ok === null) ok = await run($, ['convert', path, '-resize', tw + 'x' + th, '-quality', String(quality), '-background', 'white', '-flatten', out])
  return ok !== null
}

async function base64Of($, path) {
  let b = await run($, ['base64', '-i', path])          // macOS
  if (b === null) b = await run($, ['base64', '-w', '0', path])   // GNU
  return b ? b.replace(/\s+/g, '') : null
}

export function register(on, options) {
  maxEdge = Number((options && options.max_edge) || 1024)
  quality = Number((options && options.quality) || 85)
  minBytes = Number((options && options.min_bytes) || 60000)

  on('tool.describe', { tool: 'Read' }, async ($, e, next) => {
    const r = await next(e)
    const add = ' Images with a longest edge over ' + maxEdge + ' px are returned downscaled to that edge (the size a model reads anyway); the file on disk is unchanged.'
    return { ...r, description: ((r && r.description) || '') + add }
  })

  on('tool.call', { tool: 'Read' }, async ($, e, next) => {
    const path = e.file_path
    if (!isImagePath(path)) return next(e)
    const p = await probe($, path)
    if (!p) return next(e)
    const [w, h, bytes] = p
    if (!shouldShrink(w, h, bytes, maxEdge, minBytes)) return next(e)
    const [tw, th] = targetDims(w, h, maxEdge)
    const tmp = ((await $.env.get('TMPDIR')) || '/tmp').replace(/\/$/, '') + '/lens-' + Date.now() + '-' + Math.floor(Math.random() * 1e6) + '.jpg'
    const ok = await shrink($, path, tw, th, tmp)
    if (!ok) return next(e)
    const b64 = await base64Of($, tmp)
    const outBytes = b64 ? Math.floor(b64.length * 3 / 4) : 0
    await run($, ['rm', '-f', tmp])
    if (!b64) return next(e)
    turn.images += 1
    turn.tokensKept += Math.max(0, estTokens(w, h) - estTokens(tw, th))
    return {
      result: { type: 'image', file: { base64: b64, type: 'image/jpeg', originalSize: bytes, dimensions: { originalWidth: w, originalHeight: h, displayWidth: tw, displayHeight: th } } },
      context: [keptLine(w, h, tw, th, bytes, outBytes)],
    }
  }).catch(async ($, e, next) => next(e))

  on('turn.complete', async ($, e, next) => {
    const t = { ...turn }
    total.images += t.images
    total.tokensKept += t.tokensKept
    total.turns += 1
    turn = { images: 0, tokensKept: 0 }
    const r = await next(e)
    if (!t.images) return r
    const line = 'lens: ' + t.images + ' image' + (t.images === 1 ? '' : 's') + ' downscaled · ~' + t.tokensKept + ' tokens kept out'
    return { ...r, text: ((r && r.text) ? r.text + '\n' : '') + line }
  })
}
