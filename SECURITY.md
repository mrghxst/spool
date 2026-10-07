# Security

## Reporting a vulnerability

Please report security problems privately through
[GitHub security advisories](https://github.com/mrghxst/spool/security/advisories/new)
rather than in a public issue. Include what you found, how to reproduce it,
and what an attacker could do with it. You'll get a reply within a week.

## What's in scope

- **The relay** (`crates/relay`): anything that lets it carry plaintext,
  reach hosts or ports outside its allowlist, reach private or loopback
  addresses, exceed its connection limits, or write logs or files.
- **The engine** (`crates/engine`): TLS certificate validation, anything
  that lets the client send an NNTP command outside the read-only set
  (`AUTHINFO`, `BODY`, `STAT`, `DATE`, `CAPABILITIES`, `QUIT`), memory safety
  in parsers (NZB, yEnc, PAR2).
- **The web app** (`apps/web`): credential leaks, requests to third parties,
  path traversal when writing or extracting files, Content Security Policy
  bypasses.

## Design notes

- TLS runs in the browser and is verified against Mozilla's root store with
  SNI. A cargo feature (`test-ca`) can trust an extra root for the test
  harness. Release builds never enable it, and CI checks the deployed WASM
  for the hook.
- The relay requires the first WebSocket message to be a TLS handshake
  record, and resolves DNS itself, refusing addresses that aren't globally
  routable.
- Extracted paths are sanitised: absolute paths and `..` segments are
  rejected.

See [docs/protocol.md](docs/protocol.md) and [docs/privacy.md](docs/privacy.md).
