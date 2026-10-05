// Relay URLs and the relay's info endpoint.

export type RelayInfo = {
  name: string;
  version: string;
  protocol: number;
  ports: number[];
  allow: string[];
};

/** Normalises user input: adds wss:// when missing, drops trailing slashes. */
export function normalizeRelay(input: string): string {
  let s = input.trim();
  if (!s) return '';
  if (!/^[a-z]+:\/\//i.test(s)) {
    const local = /^(localhost|127\.0\.0\.1|\[::1\])(:|$)/i.test(s);
    s = `${local ? 'ws' : 'wss'}://${s}`;
  }
  s = s.replace(/^https:/i, 'wss:').replace(/^http:/i, 'ws:');
  return s.replace(/\/+$/, '');
}

/** True for wss:// URLs and for ws:// to this machine. */
export function relayAllowed(url: string): boolean {
  try {
    const u = new URL(url);
    if (u.protocol === 'wss:') return true;
    if (u.protocol !== 'ws:') return false;
    return ['localhost', '127.0.0.1', '[::1]'].includes(u.hostname);
  } catch {
    return false;
  }
}

export function tunnelUrl(relay: string, host: string, port: number): string {
  return `${relay}/v1/tcp/${encodeURIComponent(host.trim().toLowerCase())}/${port}`;
}

export function infoUrl(relay: string): string {
  return `${relay.replace(/^wss:/i, 'https:').replace(/^ws:/i, 'http:')}/`;
}

export async function fetchRelayInfo(relay: string, timeoutMs = 6000): Promise<RelayInfo> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(infoUrl(relay), {
      signal: ctrl.signal,
      cache: 'no-store',
      credentials: 'omit',
      referrerPolicy: 'no-referrer',
    });
    if (!res.ok) throw new Error(`The relay answered with HTTP ${res.status}.`);
    const info = (await res.json()) as RelayInfo;
    if (info.name !== 'spool-relay' || typeof info.protocol !== 'number') {
      throw new Error("That address doesn't look like a Spool relay.");
    }
    if (info.protocol !== 1) {
      throw new Error(`The relay speaks protocol ${info.protocol}; this app needs protocol 1.`);
    }
    return info;
  } catch (e) {
    if ((e as Error).name === 'AbortError') throw new Error("The relay didn't answer in time.");
    if (e instanceof TypeError) throw new Error("Couldn't reach the relay. Check the address.");
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

/** Glob match as the relay does it: `*`, `*.example.com` (subdomains), exact. */
export function hostMatches(pattern: string, host: string): boolean {
  const p = pattern.trim().toLowerCase().replace(/\.$/, '');
  const h = host.trim().toLowerCase().replace(/\.$/, '');
  if (p === '*') return true;
  if (p.startsWith('*.')) {
    const suffix = p.slice(1);
    return h.length > suffix.length && h.endsWith(suffix);
  }
  return p === h;
}

/** Explains a relay close code in plain words. */
export function describeClose(code: number, host: string, port: number): string {
  switch (code) {
    case 4001:
      return `The relay doesn't allow ${host}:${port}. Use a relay that allows it, or run your own.`;
    case 4002:
      return `The relay couldn't reach ${host}:${port}. Check the server name and port.`;
    case 4003:
      return 'The relay is at its connection limit. Try fewer connections or another relay.';
    case 4004:
      return 'The relay rejected the connection because it was not encrypted.';
    case 1006:
      return "Couldn't reach the relay. Check its address in Settings.";
    default:
      return `The connection closed (${code}).`;
  }
}
