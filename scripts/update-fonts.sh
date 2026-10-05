#!/usr/bin/env bash
# Refresh the self-hosted Geist fonts from the `geist` npm package (SIL OFL 1.1).
# The fonts are committed so the app never makes third-party requests.
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && npm pack geist --silent >/dev/null && tar xzf geist-*.tgz)
cp "$tmp/package/dist/fonts/geist-sans/Geist-Variable.woff2" apps/web/public/fonts/
cp "$tmp/package/dist/fonts/geist-mono/GeistMono-Variable.woff2" apps/web/public/fonts/
cp "$tmp/package/LICENSE.txt" apps/web/public/fonts/OFL.txt
echo "Fonts updated."
