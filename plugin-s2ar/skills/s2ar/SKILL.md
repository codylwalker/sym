---
name: s2ar
description: How the s2ar tools are paid for and what every answer carries — free samples, the $5 pack, USDC per call with x402_payment, receipts and records, free verification. Read when a tool answers "payment required" or the user asks what this costs.
disable-model-invocation: true
---

# s2ar: the house rules in one screen

- **Free samples.** Without a key each tool answers a few calls a day (three; one for certify) on its plain route.
  After that the tool answers with the price and how to pay — nothing was charged.
- **A key.** A $5, $25 or $100 pack (the links are in any "payment required" answer, or at
  https://s2ar.dev/mcp.html) shows a key once; put it in the plugin's `api_key` setting (`/config`) and calls draw
  on the balance. Signing in by email at https://api.s2ar.dev/login grants $0.50 to start.
- **USDC per call.** Every paid tool takes an `x402_payment` argument: the base64 payload a wallet signs for the
  `accepts[0]` a "payment required" answer carries (USDC on Base). The server verifies, does the work, settles only
  once the answer exists, and returns the settlement.
- **Every paid answer carries a receipt** (`receipt.evidence_hash`, the cost, the determinism of the route) and,
  for assert, inspect, score and certify, a **signed record** anyone can verify free at
  `https://api.s2ar.dev/v1/verify/<record_sha256>`. The text you sent is never kept.
- **A failed call is never charged.** The rules are at https://api.s2ar.dev/charter; each day's counts are signed at
  https://api.s2ar.dev/.well-known/proceedings.json.
- **Hooks never spend money.** The lines this plugin adds under an answer are reminders; a paid call is always a
  tool you see.
