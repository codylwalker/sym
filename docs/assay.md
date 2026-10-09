# Assay — the standard (assay/2)

An assay is a measurement of content, never an opinion. Every tool at s2ar that
measures something (starlens's image compression, starscry's reward-integrity
audits, sym's answer-agreement judge) issues the same record, and anyone can
verify one for free, forever, without an account. The record keeps hashes,
dimensions and verdicts, never pixels, prompts, or a buyer's identity.

## The record

One JSON object in two layers.

**Sealed** (`record_sha256` = sha256 of the canonical form of these fields minus
`record_sha256`): `v` (2), `issuer` (`assay` when signed by the issuer's key,
`self` otherwise), `kind`, `subject[]` (`{role, sha256, bytes?, w?, h?, media?}`:
every input artifact by hash), `harness {name, version}`, `seed`, `witnesses[]`
(`{name, deterministic, version? | model?}`), `verdict` (the kind's own word),
`outcome` (`pass | partial | fail | measured`, the one tri-state every badge and
router reads), `replayable` (true only when every witness is deterministic),
`payload` (the kind's measurement), `hash_alg` (absent means sha256).

**Signed** (outside the seal; the Ed25519 signature covers the canonical record
minus `signature`, so it embeds the seal): `issued_at`, `expires_at` (null when
the kind never expires), `status_at_issue`, `public_key`, `signature`. Live
status (`valid | stale | expired | superseded`) is the registry's answer, never
the document's.

**Canonical form**: JSON with sorted keys, separators `,` and `:`, UTF-8 kept
(`ensure_ascii` false). Unknown fields make verification fail closed.

## Verify

`GET /v1/verify/{record_sha256}` (free, rate-limited) returns `{found, seal_ok,
signed, signature_ok, key_pinned, replayable, outcome, expires_at, expired,
reason, certificate}`. It recomputes the seal, checks the signature against the
carried key and against the issuer's published key
(`GET /.well-known/assay.json`), and reports expiry. It never re-measures: the
full re-run is the client's, from the hashed artifacts it holds.

## Kinds

| kind | verdict | outcome rule | witnesses | expires |
|---|---|---|---|---|
| `compression` | `isomorphic` / `moved` (drift ≤ tolerance) | pass / fail | recon, phash, ocr (deterministic); vlm (model in loop) | never: two byte strings under a pinned target profile; a superseded profile is reported, not expired |
| `reward-integrity` | a letter grade | A, B pass; C partial; else fail | the verifiers battery (version pinned) | 180 days: the verifiers and the environment wheel move |
| `answer-agreement` | `agree` / `partial` / `disagree` / `unclear` | pass / partial / fail / measured | a judge model (`deterministic: false`); the badge says "judged" | 90 days: the judge model moves and cannot be replayed |
| `log-trim` | `verbatim` | pass | the rule set (version pinned): every kept line is a substring of the source | never |
| `refusal` | a three-class profile | measured | spec only; not issued | 180 days or the model release |

## The badge

`GET /v1/badge/{record_sha256}.svg`: `assay · <kind> · <verdict> · YYYY-MM`.
A self-sealed record carries no wordmark (visibly uncertified); a non-replayable
kind adds "judged"; anything not valid renders grey.

## Refused

A grade is a published function of the sealed record, never an argument to
issue. Payment buys the measurement run and the mark, never the verdict: a
`fail` record is issued, signed and served identically. Renewal is a re-measure.
No position is sold anywhere a record orders a list. No retention beyond the
record. No signing without a run, no backdating, no revocation of an issued
record: verify answers for every sha ever issued.
