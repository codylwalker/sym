// Pure helpers for the image lens: which files it handles, what a model
// pays for an image, and the target size.

const EXT = /\.(png|jpe?g|gif|webp|bmp|tiff?)$/i

export function isImagePath(p) {
  return EXT.test(String(p || ''))
}

// Image blocks bill about (w×h)/750 tokens, capped near 1,600: the API
// downscales anything with a long edge over 1568 px before the model sees it.
export function estTokens(w, h) {
  const t = Math.round((w * h) / 750)
  return Math.min(t, 1600)
}

// The size after fitting the longest edge to `maxEdge` (never upscaled).
export function targetDims(w, h, maxEdge) {
  const edge = Math.max(w, h)
  if (!(edge > maxEdge)) return [w, h]
  const s = maxEdge / edge
  return [Math.max(1, Math.round(w * s)), Math.max(1, Math.round(h * s))]
}

export function shouldShrink(w, h, bytes, maxEdge, minBytes) {
  return Math.max(w, h) > maxEdge && bytes >= minBytes
}

// The line the model reads after the image.
export function keptLine(w, h, tw, th, bytes, outBytes) {
  return '[lens] image downscaled ' + w + 'x' + h + ' → ' + tw + 'x' + th + ' (' + Math.round(bytes / 1024) + ' KB → ' + Math.round(outBytes / 1024) + ' KB), ~' + estTokens(tw, th) + ' tokens (was ~' + estTokens(w, h) + '). The original is unchanged on disk.'
}

// Parse `sips -g pixelWidth -g pixelHeight` or `magick identify` output.
export function parseDims(text) {
  const s = String(text || '')
  const w = /pixelWidth:\s*(\d+)/.exec(s)
  const h = /pixelHeight:\s*(\d+)/.exec(s)
  if (w && h) return [Number(w[1]), Number(h[1])]
  const m = /^\s*(\d+)\s+(\d+)\s*$/m.exec(s)
  if (m) return [Number(m[1]), Number(m[2])]
  return null
}
