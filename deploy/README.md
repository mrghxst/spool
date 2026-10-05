# Run a relay

The relay is a single static binary (about 1 MB) that forwards encrypted bytes
between the browser and your Usenet provider. It can't read them. It keeps no
logs and stores nothing.

## On your own computer

```sh
docker run -d --name spool-relay --restart unless-stopped -p 8080:8080 ghcr.io/mrghxst/spool-relay
```

In Spool, open Settings and set the relay to `ws://localhost:8080`. Browsers
allow `ws://localhost` from an https page, so this works with the hosted app.

No Docker? Download `spool-relay-linux-amd64` (or `-arm64`) from the
[releases page](https://github.com/mrghxst/spool/releases) and run it.

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
[docs/protocol.md](../docs/protocol.md#configuration). Common ones:

- `SPOOL_ALLOW="*"` allows any public Usenet host instead of the curated list.
- `SPOOL_ORIGINS=https://you.github.io` only accepts your copy of the app.
- `SPOOL_MAX_CONNS_PER_IP=32` lowers the per-client limit.

## Updating

```sh
sudo docker compose pull && sudo docker compose up -d
```

## Build the image yourself

```sh
git clone https://github.com/mrghxst/spool && cd spool
docker build -f crates/relay/Dockerfile -t spool-relay .
```
