# Self-hosting

The relay is a single static binary (about 1 MB) that forwards encrypted bytes
between the browser and your Usenet provider. It can't read them. It keeps no
logs and stores nothing.

Spool uses the public relay at `relay.itsanon.com` unless you choose another
in Settings. Running your own is the fastest option, and the files for it are
in [`deploy/`](https://github.com/mrghxst/spool/tree/main/deploy).

## On your own computer

On Windows or macOS, download the binary for your system from the
[releases page](https://github.com/mrghxst/spool/releases/latest) and run it.
Without settings it listens on `127.0.0.1:8080` (this computer only) and
allows any public Usenet host. Set the relay in Spool to `ws://localhost:8080`.
`spool-relay local` does the same on Linux.

With Docker:

```sh
docker run -d --name spool-relay --restart unless-stopped -p 8080:8080 ghcr.io/mrghxst/spool-relay
```

In Spool, open Settings and set the relay to `ws://localhost:8080`. Browsers
allow `ws://localhost` from an https page, so this works with the hosted app.

No Docker? Download `spool-relay-linux-amd64` (or `-arm64`) from the
[releases page](https://github.com/mrghxst/spool/releases) and run it.

## On a home server or NAS

Spool runs on an https page, and browsers only allow plain `ws://` to the
same computer. A relay on another machine, such as `192.168.1.30:8080`, needs
HTTPS in front of it so you can use `wss://`. Typing `ws://192.168.1.30:8080`
or `https://192.168.1.30:8080` won't work: the relay itself doesn't speak TLS.

**If you already run a reverse proxy** (Caddy, Traefik, Nginx Proxy Manager,
SWAG) with certificates for your domain, add a host such as
`relay.home.example.com` that forwards to the relay container on port 8080
with WebSockets enabled. Then use `wss://relay.home.example.com`.

**If you don't**, Caddy can get a certificate for a LAN-only name with the
DNS challenge. This example uses Cloudflare DNS:

1. Create an `A` record `relay.home.example.com` pointing at the server's LAN
   address (`192.168.1.30`), **DNS only**. Create a Cloudflare API token with
   *Zone → DNS → Edit* for that zone.
2. Build Caddy with the Cloudflare DNS module:

   ```dockerfile
   # Dockerfile
   FROM caddy:2-builder AS build
   RUN xcaddy build --with github.com/caddy-dns/cloudflare
   FROM caddy:2-alpine
   COPY --from=build /usr/bin/caddy /usr/bin/caddy
   ```

3. Compose file and Caddyfile next to it:

   ```yaml
   services:
     relay:
       image: ghcr.io/mrghxst/spool-relay:latest
       restart: unless-stopped
       environment:
         SPOOL_TRUST_PROXY: "1"
     caddy:
       build: .
       restart: unless-stopped
       ports: ["443:443"]
       environment:
         CF_API_TOKEN: your-token
       volumes:
         - ./Caddyfile:/etc/caddy/Caddyfile:ro
         - caddy_data:/data
   volumes:
     caddy_data:
   ```

   ```
   relay.home.example.com {
   	tls {
   		dns cloudflare {env.CF_API_TOKEN}
   	}
   	reverse_proxy relay:8080
   }
   ```

4. `docker compose up -d --build`, then set the relay to
   `wss://relay.home.example.com` in Spool. Chrome may ask whether the page
   can reach devices on your local network; allow it.

A Cloudflare Tunnel also works and reaches the relay from anywhere, but all
download traffic then flows through Cloudflare, which is slower and may
conflict with their terms for large transfers.

The relay only allows the providers in its built-in list. If yours isn't on
it, set `SPOOL_ALLOW: "*"`.

## On a VPS in five minutes (Debian 12 or 13)

You need a VPS with a public IPv4 address and a domain name.

1. **DNS.** Create an `A` record such as `relay.example.com` pointing at the
   VPS. If you use Cloudflare, set it to **DNS only** (grey cloud), not
   proxied.

2. **Docker.** Install Docker Engine with the Compose plugin:

   ```sh
   curl -fsSL https://get.docker.com | sudo sh
   ```

3. **Files.**

   ```sh
   mkdir spool-relay && cd spool-relay
   curl -fsSLO https://raw.githubusercontent.com/mrghxst/spool/main/deploy/docker-compose.yml
   curl -fsSLO https://raw.githubusercontent.com/mrghxst/spool/main/deploy/Caddyfile
   sed -i 's/relay.example.com/relay.yourdomain.com/' Caddyfile
   ```

4. **Start.**

   ```sh
   sudo docker compose up -d
   ```

   Caddy gets a certificate from Let's Encrypt automatically. Ports 80 and 443
   must be reachable.

5. **Check.** `curl https://relay.yourdomain.com/healthz` prints `ok`. In
   Spool, open Settings, set the relay to `wss://relay.yourdomain.com` and
   click **Test relay**.

## Logging

The relay prints one startup line and nothing else. Caddy writes no access
logs unless you add a `log` directive to the Caddyfile, so don't add one.
Docker keeps the container's stdout and stderr, which holds only that startup
line.

## Options

All options are environment variables; see
[Protocol](./protocol.md#configuration). Common ones:

- `SPOOL_ALLOW="*"` allows any public Usenet host instead of the curated list.
- `SPOOL_ORIGINS=https://you.github.io` only accepts your copy of the app.
- `SPOOL_MAX_CONNS_PER_IP=32` lowers the per-client limit.

## Updating

```sh
sudo docker compose pull && sudo docker compose up -d
```

To update automatically, let a systemd timer check for new releases. The
`latest` tag only moves when a version is released, so the relay doesn't
pick up untested changes from `main` (that's the `edge` tag). This assumes
the compose files are in `/opt/spool-relay`.

`/usr/local/bin/spool-relay-update` (make it executable):

```sh
#!/bin/sh
cd /opt/spool-relay || exit 1
docker compose pull -q && docker compose up -d --remove-orphans && docker image prune -f >/dev/null
```

`/etc/systemd/system/spool-relay-update.service`:

```ini
[Unit]
Description=Update spool-relay and Caddy images
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/spool-relay-update
```

`/etc/systemd/system/spool-relay-update.timer`:

```ini
[Unit]
Description=Check for new spool-relay releases every 15 minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=15min
RandomizedDelaySec=60

[Install]
WantedBy=timers.target
```

Then `sudo systemctl daemon-reload && sudo systemctl enable --now
spool-relay-update.timer`. Containers are only recreated when an image
changed, which drops open downloads for a moment.

::: warning Firewalls and Docker
On Debian, restarting the stock `nftables` service runs `nft flush ruleset`,
which also deletes Docker's rules and cuts the containers off from the
internet. Keep your own rules in a separate table and load them with
`nft -f`, without a flush.
:::

## Build the image yourself

```sh
git clone https://github.com/mrghxst/spool && cd spool
docker build -f crates/relay/Dockerfile -t spool-relay .
```
