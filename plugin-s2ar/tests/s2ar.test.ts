import { s2arTool, carriesJson, x402Urls, reminder, contextBlock } from '../hooks/scan.js'
import { expect, test } from 'claude-code/testing'

test('the server tools are recognised by their Claude Code ids', async () => {
  expect(s2arTool('mcp__s2ar_api__assert_output')).toBe('assert_output')
  expect(s2arTool('mcp__s2ar_api__inspect_x402')).toBe('inspect_x402')
  expect(s2arTool('mcp__sym__map')).toBe(null)
  expect(s2arTool('Read')).toBe(null)
})

test('an answer carrying JSON is recognised; prose with braces is not', async () => {
  expect(carriesJson('Here it is:\n```json\n{"a": 1}\n```')).toBe(true)
  expect(carriesJson('{"summary": "x", "source_url": "https://e.example"}')).toBe(true)
  expect(carriesJson('[{"id": 1}, {"id": 2}]')).toBe(true)
  expect(carriesJson('Use {braces} for templates; the map {} is empty.')).toBe(false)
  expect(carriesJson('')).toBe(false)
})

test('x402 endpoints named in an answer are found', async () => {
  expect(x402Urls('pay https://api.s2ar.dev/x402/v1/assert for it.')).toEqual(['https://api.s2ar.dev/x402/v1/assert'])
  expect(x402Urls('an x402 seller at https://x.example/v1/thing, nothing else')).toEqual(['https://x.example/v1/thing'])
  expect(x402Urls('see https://docs.example/page')).toEqual([])
})

test('the reminder fires only when the work was not done, and respects off', async () => {
  const json = '```json\n{"a": 1, "b": 2}\n```'
  expect(reminder(json, {}, { reminders: 'on' })).toMatch(/not asserted/)
  expect(reminder(json, { assert_output: 1 }, { reminders: 'on' })).toBe(null)
  expect(reminder(json, {}, { reminders: 'off' })).toBe(null)
  const url = 'pay https://api.s2ar.dev/x402/v1/assert'
  expect(reminder(url, {}, { reminders: 'on' })).toMatch(/inspect_x402/)
  expect(reminder(url, { inspect_x402: 1 }, { reminders: 'on' })).toBe(null)
  expect(reminder('plain prose', {}, { reminders: 'on' })).toBe(null)
  const both = reminder(json + '\n' + url, {}, { reminders: 'on' }) || ''
  expect(both.includes('not asserted') && both.includes('inspect_x402')).toBe(true)
})

test('the context block says how paying works, with and without a key', async () => {
  expect(contextBlock(false).text).toMatch(/no key is configured/)
  expect(contextBlock(true).text).toMatch(/a key is configured/)
  expect(contextBlock(false).name).toBe('s2ar-tools')
})
