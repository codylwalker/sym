# Deploying the hosted tier (staros-work1, per docs/hosting.md)

Three processes, all loopback except the public Caddy vhost:

1. `sym serve` (this repo, `deploy/sym-serve.service`): build `cargo build --release`
   on the box or copy the Linux binary to `/usr/local/bin/sym`; the cache dir
   `/var/lib/starlens/sym-cache` is owned by the starlens user (the gateway
   clones into it; sym only reads).
2. The starlens service already running on `:8471` picks up `/v1/sym/*` on its
   next deploy of the starlens repo (routes in `src/starlens/symsvc.py`). New env
   in `/etc/starlens.env`: `STARLENS_SYM_CACHE=/var/lib/starlens/sym-cache`,
   `STARLENS_SYM_URL=http://127.0.0.1:8431`, prices if not default, and the
   rails: `STARLENS_X402_PAY_TO` (x402), `STARLENS_MPP_SIDECAR=http://127.0.0.1:8433`.
3. The MPP sidecar (`starlens/deploy/mpp-sidecar`, `mpp-sidecar.service`): `npm ci`,
   `/etc/starlens-mpp.env` with `STRIPE_SECRET_KEY`, `STRIPE_PROFILE_ID`, optional
   `TEMPO_DEPOSIT_ADDRESS`. Sandbox first (`sk_test_`, `profile_test_`).

4. The embedder behind `/v1/sym/where` (`rust-shared/staros-embed`, unit
   `staros-embed/deploy/staros-embed.service`): on work1 it runs on the CPU with
   the int8 Qwen3-Embedding-0.6B export (`~/models/qwen3-embed-0.6b`), loading an
   official ONNX Runtime wheel through `ORT_DYLIB_PATH` (the crate's
   `dynamic-cpu` feature; pyke's downloaded binaries want a newer glibc than
   Debian 12 has). Loopback `:8440`; `STARLENS_EMBED_URL=http://127.0.0.1:8440`
   in `/etc/starlens.env`. ~0.8 s per 450-token chunk, so a repository the size
   of ripgrep (3.7k chunks) indexes in about 50 minutes the first time; the
   first `where` call answers 202 until then and nothing is charged.

Caddy: `starlens/deploy/sites/sym.caddyfile` → `/etc/caddy/sites/`, then
`systemctl reload caddy`. DNS: `s2ar.dev/sym` A record to the box. Site files:
`tools/deploy_site.py` scp's `site/` to `/srv/stardata/site/sym/`.

Checks: `curl -s https://s2ar.dev/sym/healthz`; `curl -s -X POST
https://api.s2ar.dev/v1/sym/map -H 'content-type: application/json' -d
'{"repo":"https://github.com/BurntSushi/ripgrep","ref":"14.1.1"}'` → 402 with
every configured rail; `npx mppx@latest validate https://api.s2ar.dev/v1/sym/map`.
