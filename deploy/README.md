# Run a relay

The guide is on the documentation site:
**[Self-hosting](https://mrghxst.github.io/spool/self-hosting)**
(source: [docs/self-hosting.md](../docs/self-hosting.md)).

This folder has the files it uses:

- [`docker-compose.yml`](docker-compose.yml): the relay behind Caddy, for a
  VPS with a domain name.
- [`Caddyfile`](Caddyfile): HTTPS for that setup. Change `relay.example.com`
  to your domain.
