# FAQ

## Why is there a relay at all?

Web pages can't open raw TCP connections.
The relay turns a WebSocket into a TCP connection and nothing more. The
browser does the TLS handshake with your provider through it, so the relay
sees only encrypted bytes. The relay also refuses any connection that doesn't
start with a TLS handshake.

## Can the relay operator see my password?

No. Your login happens inside
the TLS session between your browser and the provider.

## Which providers work?

Any provider with TLS on port 563 or 443. The
public relay allows a list of common provider domains; your own relay can
allow anything (`SPOOL_ALLOW=*`).

## Does all my download traffic go through someone's server?

Only if you
use a relay on another machine. Browsers can't open connections to Usenet
servers themselves, so something has to turn a WebSocket into a TCP
connection. Run the relay on your own computer ([see Self-hosting](./self-hosting.md#on-your-own-computer)) and nothing else is in
the path: the bytes go from your provider to the relay on your machine and
into the tab. A relay on your home server is just as fast over your LAN. The
public relay is for computers where you can't run anything.

## How fast is it?

On a desktop, against a fast relay, about 65 to 75 MB/s
in Chromium. Your connection and your relay's are usually the limit. See
[Decisions](./decisions.md#performance).

## Can I close the tab mid-download?

Not yet. A download needs the tab to
stay open; Spool asks before you leave.

## Encrypted archives?

Spool can't decrypt RAR or encrypted 7z/zip in the
browser. It keeps the archive files and shows the password from the NZB, so
you can extract them with a desktop tool.

## Where are my settings stored?

In this browser's IndexedDB, and only if
"Remember on this device" is on. Settings has "Forget everything".

## Why does a finished download say "Saving to disk"?

Every article has arrived, but the last data is still being written. In
Chromium, the browser also checks each file it writes into your folder before
the file appears there, which takes a few seconds per gigabyte for the last
file. Spool pauses new requests whenever more than 256 MB is waiting for the
disk, so this tail stays short.

## Can I host the app myself?

Yes, it's static files. Build it with `pnpm build` and serve
`apps/web/dist` from any static host. The repo includes a `wrangler.jsonc`
for Cloudflare Workers: connect the repo in Workers Builds with build command
`pnpm run build` and deploy command `npx wrangler deploy`. The build fetches
the Rust toolchain on Cloudflare's builder by itself.
