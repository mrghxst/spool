// net.worker: WebSockets to the relay plus the WASM engine (TLS + NNTP +
// yEnc). Decoded segments go straight to the writer as transferable buffers;
// the coordinator only gets small notifications.

import { loadEngine, type Engine } from '../engine/load';
import type { FromNet, OpenConn, ToNet, ToWriter } from './protocol';

declare const self: DedicatedWorkerGlobalScope;

type EngineConn = InstanceType<Engine['Conn']>;

type Live = {
  ws: WebSocket;
  conn: EngineConn | null;
  /** segId -> fileIdx for requests on this connection. */
  files: Map<number, number>;
  started: number;
  opened: boolean;
  closed: boolean;
  /** The coordinator asked to close; anything still arriving is dropped. */
  closing: boolean;
};

let coord: MessagePort | null = null;
let writer: MessagePort | null = null;
let testCa: Uint8Array | null = null;
const conns = new Map<number, Live>();

function toCoord(msg: FromNet) {
  coord?.postMessage(msg);
}

function toWriter(msg: ToWriter, transfer: Transferable[] = []) {
  writer?.postMessage(msg, transfer);
}

function flush(live: Live) {
  if (!live.conn || live.ws.readyState !== WebSocket.OPEN) return;
  for (;;) {
    const out = live.conn.take_outgoing();
    if (!out) break;
    live.ws.send(out as Uint8Array<ArrayBuffer>);
  }
}

function finish(id: number, live: Live, reason: string, wsCode: number) {
  if (live.closed) return;
  live.closed = true;
  conns.delete(id);
  try {
    live.conn?.free();
  } catch {
    // Already freed.
  }
  live.conn = null;
  toCoord({ type: 'closed', connId: id, reason, wsCode, opened: live.opened });
}

type EngineEvent =
  | { type: 'ready'; posting: boolean }
  | { type: 'authOk' }
  | { type: 'authFailed'; code: number }
  | {
      type: 'segment';
      segId: number;
      begin: number;
      data: Uint8Array;
      crcOk: boolean;
      name: string;
      fileSize: number;
      part: number;
    }
  | { type: 'missing'; segId: number }
  | { type: 'busy'; code: number }
  | { type: 'closed'; reason: string }
  | { type: 'stat' | 'date' | 'capabilities' };

function drain(id: number, live: Live) {
  const conn = live.conn;
  if (!conn) return;
  for (;;) {
    const ev = conn.poll_event() as EngineEvent | undefined;
    if (!ev) break;
    const ms = Math.round(performance.now() - live.started);
    switch (ev.type) {
      case 'ready':
        toCoord({ type: 'ready', connId: id, ms });
        break;
      case 'authOk':
        toCoord({ type: 'auth-ok', connId: id, ms });
        break;
      case 'authFailed':
        toCoord({ type: 'auth-failed', connId: id, code: ev.code });
        break;
      case 'busy':
        toCoord({ type: 'busy', connId: id, code: ev.code });
        break;
      case 'missing':
        if (live.closing) break;
        live.files.delete(ev.segId);
        toCoord({ type: 'missing', connId: id, segId: ev.segId });
        break;
      case 'segment': {
        // The coordinator already gave this connection's articles to others.
        if (live.closing) break;
        const fileIdx = live.files.get(ev.segId) ?? -1;
        live.files.delete(ev.segId);
        const bytes = ev.data.length;
        if (ev.crcOk && bytes > 0 && fileIdx >= 0) {
          const buf = ev.data.buffer as ArrayBuffer;
          toWriter(
            { type: 'write', fileIdx, segId: ev.segId, offset: ev.begin, data: buf },
            [buf],
          );
        }
        toCoord({
          type: 'segment',
          connId: id,
          segId: ev.segId,
          fileIdx,
          bytes,
          begin: ev.begin,
          crcOk: ev.crcOk && bytes > 0,
          name: ev.name,
          fileSize: ev.fileSize,
        });
        break;
      }
      case 'closed':
        try {
          live.ws.close(1000);
        } catch {
          // Ignore.
        }
        finish(id, live, ev.reason, 1000);
        return;
      default:
        break;
    }
  }
}

async function open(msg: OpenConn) {
  const engine = await loadEngine(testCa);
  let conn: EngineConn;
  try {
    conn = new engine.Conn(msg.host, msg.user, msg.pass);
  } catch (e) {
    toCoord({ type: 'closed', connId: msg.connId, reason: String((e as Error).message ?? e), wsCode: 0, opened: false });
    return;
  }
  let ws: WebSocket;
  try {
    ws = new WebSocket(msg.url);
  } catch {
    conn.free();
    toCoord({ type: 'closed', connId: msg.connId, reason: 'invalid relay address', wsCode: 1006, opened: false });
    return;
  }
  ws.binaryType = 'arraybuffer';
  const live: Live = { ws, conn, files: new Map(), started: performance.now(), opened: false, closed: false, closing: false };
  conns.set(msg.connId, live);
  ws.onopen = () => {
    live.opened = true;
    flush(live);
  };
  ws.onmessage = (e: MessageEvent) => {
    if (!live.conn || !(e.data instanceof ArrayBuffer)) return;
    live.conn.on_bytes(new Uint8Array(e.data));
    drain(msg.connId, live);
    flush(live);
  };
  ws.onclose = (e: CloseEvent) => {
    if (live.closed) return;
    live.conn?.transport_closed(e.reason || `connection closed (${e.code})`);
    // Deliver anything decoded before the close, then report it.
    drain(msg.connId, live);
    finish(msg.connId, live, e.reason || '', e.code);
  };
}

function request(connId: number, items: [number, number, string][]) {
  const live = conns.get(connId);
  if (!live?.conn) return;
  for (const [segId, fileIdx, msgid] of items) {
    live.files.set(segId, fileIdx);
    live.conn.request(segId, msgid);
  }
  drain(connId, live);
  flush(live);
}

function close(connId: number) {
  const live = conns.get(connId);
  if (!live) return;
  live.closing = true;
  try {
    live.conn?.quit();
    flush(live);
  } catch {
    // Ignore.
  }
  setTimeout(() => {
    try {
      live.ws.close(1000);
    } catch {
      // Ignore.
    }
    finish(connId, live, 'closed', 1000);
  }, 50);
}

// The UI wires this worker up once: port 0 talks to the coordinator,
// port 1 to the writer.
self.onmessage = (e: MessageEvent<{ type: 'wire'; testCa: Uint8Array | null }>) => {
  if (e.data?.type !== 'wire') return;
  [coord, writer] = e.ports;
  testCa = e.data.testCa;
  coord.onmessage = (m: MessageEvent<ToNet>) => {
    const msg = m.data;
    switch (msg.type) {
      case 'open':
        void open(msg);
        break;
      case 'request':
        request(msg.connId, msg.items);
        break;
      case 'close':
        close(msg.connId);
        break;
    }
  };
};
