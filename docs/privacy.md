# Privacy

Spool is a static web page, a relay that forwards encrypted bytes, and your
own Usenet account. This is what each part can see.

| Party | Sees | Never sees | Keeps |
| --- | --- | --- | --- |
| **Static host** (Cloudflare for the app, GitHub Pages for these docs) | Your IP address and the files you load | Anything you do in the app | Standard request logs, under Cloudflare's and GitHub's policies. Spool keeps none |
| **Relay** | Your IP address, the provider hostname and port, byte counts while connected | Your username, password, message-ids, file names or content. TLS runs from your browser to the provider | Nothing. No logs, no disk. In-memory connection counters only, dropped when you disconnect |
| **Usenet provider** | The relay's IP address, your account, what you download | Your IP address (it sees the relay's) | Whatever any newsreader would leave, under your provider's terms |
| **Your browser** | Everything | | Providers and settings in IndexedDB, only when "Remember on this device" is on |

## In detail

**Credentials.** Usernames and passwords are stored in IndexedDB on your
device, and only when "Remember on this device" is on (the default). With it
off, they live in memory and disappear when you close the tab, which suits
shared or school computers. "Forget everything" in Settings deletes IndexedDB,
staged files in the origin private file system, and the offline cache.

**End-to-end TLS.** The browser runs rustls (compiled to WebAssembly) and
verifies the provider's certificate against Mozilla's root store with SNI. The
relay forwards the encrypted bytes and refuses any connection whose first
message isn't a TLS handshake, so it never carries plaintext credentials.

**No third parties.** The app loads nothing from other origins: fonts are
self-hosted and there are no analytics, telemetry or error reporting. The only
network connections are to the relay you configure. The page's Content
Security Policy enforces this.

**Downloads.** On Chromium browsers files are written straight into the folder
you choose. On Firefox and Safari they're staged in the browser's private file
system until you click "Save files", then removed.

**Running your own relay.** If you'd rather not trust anyone else's relay with
your IP address and the provider you use, run one on your own machine
(`docker run -p 8080:8080 ghcr.io/mrghxst/spool-relay`, then set the relay to
`ws://localhost:8080`) or on a server you control. See
[Self-hosting](./self-hosting.md).

**Exported settings** contain your passwords in plain text. Keep that file
private.
