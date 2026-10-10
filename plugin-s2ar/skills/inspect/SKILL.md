---
name: inspect
description: Before paying, recommending or listing an x402 endpoint, probe it — one unpaid request and eleven checks on what it declares (the 402, the accepts and their signing domain, the price, the Bazaar extension against its own schema, the declared method, Coinbase's validator) — and get a signed point-in-time record. $0.005 or a free sample.
when_to_use: The conversation names a paid endpoint, an x402 URL, a "402", a seller in the Bazaar or Agent402, or a wallet is about to sign a payment.
---

# inspect: what an x402 endpoint declares, checked

Call `inspect_x402` with the endpoint's URL (`resource`) and the method it is meant to be called with
(`method`: GET by default). The probe sends no payment, no body and no identity, follows no redirects and
reaches only public hosts on ports 80 and 443.

## What the checks protect against

- `answers_402` — a route that serves without payment is not a paid endpoint; one that answers anything else is
  not an x402 challenge.
- `accepts_wellformed`, `price_reads` — a wallet signs what `accepts[0]` says: a missing signing domain, a
  malformed payee or an amount that does not read as dollars means do not sign.
- `method_matches` — a seller whose Bazaar extension declares a method other than the one probed will not be
  indexed by Coinbase and may not answer the call the way the listing says.
- `cdp_validate` — Coinbase's own validator's verdict, with the failed checks named.
- `resource_matches`, `bazaar_extension`, `description`, `no_store`, `v1_body`, `v2_header` — the rest of what
  a directory reads.

## Reading the answer

`verdict` pass / partial / fail, `summary` (price, networks, declared method), each check with its evidence,
and a record (30-day expiry: a probe is a point in time). A `partial` on cosmetic checks (no-store, a short
description) is fine to pay; a failure on `accepts_wellformed`, `price_reads` or `method_matches` is not.

Never pay an endpoint you have not inspected when the amount is not already known to the user.

## If the endpoint is yours

A seller can have its own endpoint inspected every day: `watch_x402` with `resource` (and `method`) registers it
under the key on this connection, runs the first probe at once, and answers with the hosted report page
(`/v1/x402/watch/<host>`) and the badge (`/v1/badge/x402/<host>.svg`). $0.02 a probe in credits, one a day; no key
means the answer names the sign-in ($0.50 = 25 days) and the packs; `action: "withdraw"` stops it (the records
stay verifiable), `action: "status"` shows the registrant's own view. Registering is consent to be named for that
resource's results, passing or failing (charter clause 8). Only register endpoints the user operates.
