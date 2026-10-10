---
name: assert
description: Check an output before it goes out — JSON against a schema, required or banned terms, citations present, quotations found verbatim in the source you were given — and get a signed record. Use before returning structured output, after writing an API response or a config, and whenever an answer quotes a document. One tool call, deterministic, $0.005 or a free sample.
when_to_use: The task asked for JSON, YAML-as-JSON, a schema, "structured output", a quotation with a source, a citation, or anything another program will parse.
---

# assert: the output, checked, with a record

Call `assert_output` (the s2ar MCP tool) with the output and the checks the task implies. The answer is a
verdict (`accepted`), a `score`, every check with its evidence, and a signed record anyone can verify free.
The text you send is never kept; the record holds its hash.

## Which checks

| the task | rubric or checks |
|---|---|
| "answer as JSON matching this schema" | `rubric: "api-response"`, `schema: <the schema>` |
| the answer quotes a document you were given | `rubric: "quotes-source"`, `source: <the document>` |
| the answer must cite | `rubric: "cited-answer"` (+ `domains: [...]` when the sources are known) |
| no hedging allowed | `rubric: "no-hedging"` |
| anything else | `checks: [{"check": "required_terms", "terms": [...]}, {"check": "length", "unit": "words", "max": 200}, …]` — `GET /v1/assert/checks` lists the fourteen kinds |

Rubrics and checks combine: `rubric` plus `checks` runs both.

## Reading the answer

- `accepted: true` — hand the output on. Quote the record (`record_sha256`) if the caller wants evidence.
- `accepted: false` — read `results[]`: each failed check names where (`missing`, `found[].offset`,
  `path` + `message` for a schema, `missing` quotes for the source). Fix the output, assert once more.
  Do not loop: two passes is the rule; if the second still fails, say so with the evidence.
- A `partial` verdict is a failure with some checks passing; the failed ones are the work.

## Cost

$0.005 a call; three a day free without a key; a failed check is still an answer (and charged): the evidence is
what you bought. The record of a free sample expires after 30 days; a paid one never does.
