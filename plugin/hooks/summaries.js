// One-line file summaries for the repo map (opt-in, `summaries: "haiku"`).
// Pure helpers here so they can be tested without a session; the model calls
// live in sym.js.

// The map text is `path:` header lines followed by indented signature lines.
// Returns [{ file, block }] in map order; `block` is the file's signatures.
export function filesOf(mapText) {
  const out = []
  let cur = null
  for (const line of String(mapText || '').split('\n')) {
    if (/^\S.*:\s*$/.test(line)) {
      cur = { file: line.replace(/:\s*$/, ''), block: [] }
      out.push(cur)
    } else if (cur && /^\s+\S/.test(line)) {
      cur.block.push(line.trim())
    }
  }
  return out.map((f) => ({ file: f.file, block: f.block.join('\n') }))
}

// The map with ` — <summary>` after each header that has one.
export function withSummaries(mapText, summaries) {
  return String(mapText || '')
    .split('\n')
    .map((line) => {
      if (!/^\S.*:\s*$/.test(line)) return line
      const file = line.replace(/:\s*$/, '')
      const s = summaries && summaries[file]
      return s ? file + ':  — ' + s : line
    })
    .join('\n')
}

// The prompt for one file. Short on purpose: the answer is a map annotation.
export function summaryPrompt(file, block) {
  return (
    'In at most twelve words, say what this source file is for, from its top-level signatures. ' +
    'Answer with the words only: no file name, no quotes, no trailing period.\n\n' +
    'File: ' + file + '\n' + block
  )
}

// A model answer, trimmed to one clean line.
export function cleanSummary(text) {
  const line = String(text || '').split('\n').map((l) => l.trim()).find((l) => l) || ''
  return line.replace(/^["'`]+|["'`.]+$/g, '').slice(0, 120)
}
