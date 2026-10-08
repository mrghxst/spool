# Relay protocol v1

Browsers can't open TCP sockets, so Spool tunnels each NNTP connection through
a WebSocket. TLS runs inside the browser (rustls compiled to WebAssembly) and
terminates at the Usenet provider. The relay only moves encrypted bytes.

## Endpoints

### `GET /v1/tcp/{host}/{port}` (WebSocket upgrade)

Opens a tunnel to `host:port`.

- Only binary frames are accepted. A text frame closes the tunnel with `1003`.
- The **first client frame must start with a TLS handshake record**: byte 0 is
  `0x16` (handshake), byte 1 is `0x03` and byte 2 is `0x01` to `0x04`. Any
  other first frame closes the tunnel with `4004`. The relay doesn't dial the
  destination until it has seen this frame. This guarantees the relay never
  carries plaintext credentials and can't be used as a general TCP proxy.
- After the first frame, bytes are forwarded verbatim in both directions. A
  WebSocket message may contain any number of bytes; message boundaries carry
  no meaning.
- `host` must be a DNS name or IPv4 literal that matches the relay's host
  allowlist, and `port` must be in its port allowlist.
- The relay resolves `host` itself and refuses to connect to addresses that
  aren't globally routable (loopback, RFC 1918, link-local, CGNAT `100.64/10`,
  ULA `fc00::/7`, multicast, unspecified, documentation and benchmarking
  ranges, and IPv6 transition prefixes that can embed private IPv4).
- It connects dual-stack, alternating IPv6 and IPv4 addresses, with a 10 s
  overall timeout.
- Idle tunnels (no bytes in either direction) are closed after
  `SPOOL_IDLE_SECS` with code `1000`.

#### Close codes

| Code   | Meaning                                                             |
| ------ | ------------------------------------------------------------------- |
| `1000` | Normal close: the provider closed the connection, or idle timeout    |
| `1001` | The relay is shutting down                                           |
| `1003` | A text frame was sent                                                |
| `4001` | Not allowed: origin, host, port or resolved address is not allowed  |
| `4002` | Connect failed: DNS failure, timeout or connection refused           |
| `4003` | Limit reached: too many tunnels in total or from this client IP     |
| `4004` | Not TLS: the first frame wasn't a TLS handshake record              |

### `GET /`

Returns relay information with `Access-Control-Allow-Origin: *`, so the app
can test a relay before using it:

```json
{
  "name": "spool-relay",
  "version": "0.1.0",
  "protocol": 1,
  "ports": [563, 443],
  "allow": ["*.newshosting.com", "*.eweka.nl"]
}
```

### `GET /healthz`

Returns `ok` with status 200.

## Host patterns

- `*` matches any host.
- `*.example.com` matches any subdomain of `example.com`, at any depth, but not
  `example.com` itself.
- Anything else must match exactly. Matching is case-insensitive.

## Configuration

| Variable                 | Default            | Meaning                                                    |
| ------------------------ | ------------------ | ---------------------------------------------------------- |
| `SPOOL_LISTEN`           | `0.0.0.0:8080`     | Listen address                                             |
| `SPOOL_ALLOW`            | curated list       | Host patterns, comma or space separated. `*` allows any public host |
| `SPOOL_PORTS`            | `563,443`          | Allowed destination ports                                  |
| `SPOOL_MAX_CONNS`        | `1024`             | Maximum open tunnels                                       |
| `SPOOL_MAX_CONNS_PER_IP` | `64`               | Maximum open tunnels per client IP                         |
| `SPOOL_IDLE_SECS`        | `180`              | Idle timeout in seconds                                    |
| `SPOOL_ORIGINS`          | empty (any)        | Exact `Origin` values allowed to connect                   |
| `SPOOL_TRUST_PROXY`      | `0`                | Use the right-most `X-Forwarded-For` entry as the client IP |

`spool-relay local`, the default on Windows and macOS, changes two defaults:
`SPOOL_LISTEN` becomes `127.0.0.1:8080` and `SPOOL_ALLOW` becomes `*`.
Variables that are set still win.

The curated default list covers common Usenet provider domains; see
[`crates/relay/src/config.rs`](https://github.com/mrghxst/spool/blob/main/crates/relay/src/config.rs).

## What the relay keeps

Nothing on disk. It has no logging framework and prints one line to stderr
when it starts. In memory it keeps one counter of open tunnels in total and
one per client IP, which are dropped when the tunnels close.

## Local relays

Browsers treat `ws://localhost` and `ws://127.0.0.1` as secure, so the hosted
app (served over https) can use a relay running on your own machine:

```sh
docker run --rm -p 8080:8080 ghcr.io/mrghxst/spool-relay
```

Then set the relay to `ws://localhost:8080` in Settings.
