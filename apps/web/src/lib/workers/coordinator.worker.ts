// coordinator.worker: owns the job. It parses the NZB, schedules segments
// across providers and connections, routes missing articles to the next
// provider, and drives verification, repair and extraction.

import { loadEngine } from '../engine/load';
import {
  downloadOrder,
  jobFolderName,
  nextProvider,
  pickVolumes,
  uniqueName,
  volumeBlocks,
} from '../core/plan';
import { looksObfuscated } from '../core/names';
import { describeClose, tunnelUrl } from '../core/relay';
import {
  Cell,
  type FileKind,
  type FileProgress,
  type JobResult,
  type JobSummary,
  type NzbJson,
  type Phase,
  type Progress,
  type Provider,
  type Target,
} from '../types';
import type {
  FromCoordinator,
  FromNet,
  FromPost,
  FromWriter,
  StartJob,
  ToCoordinator,
  ToNet,
  ToPost,
  ToWriter,
  VerifyResult,
} from './protocol';

declare const self: DedicatedWorkerGlobalScope;

/** Requests handed to one connection ahead of time (8 are on the wire). */
const WINDOW = 16;
/**
 * Stop requesting articles while this many received bytes wait for the disk,
 * so a slow disk can't fill memory and the progress bar tracks what's saved.
 */
const MAX_UNWRITTEN = 256 * 1024 * 1024;
/**
 * A connection with requests out and nothing back for this long is treated as
 * dead: its articles go to other connections and a new one replaces it. A
 * dropped route or a stuck server otherwise holds them forever, with the
 * socket still open.
 */
const STALL_MS = 45_000;
/** Connecting and logging in takes longer than this: give up and retry. */
const CONNECT_MS = 30_000;
/** After "too many connections", wait this long before trying one more. */
const REGROW_MS = 60_000;
const MAX_FAILURES = 3;
const TEST_TIMEOUT_MS = 20000;

let nets: MessagePort[] = [];
let writer: MessagePort;
let post: MessagePort;
let testCa: Uint8Array | null = null;
let nextConnId = 1;
let netRound = 0;

const parsed = new Map<string, NzbJson>();

function ui(msg: FromCoordinator, transfer: Transferable[] = []) {
  self.postMessage(msg, transfer);
}

function toNet(i: number, msg: ToNet) {
  nets[i].postMessage(msg);
}

function toWriter(msg: ToWriter) {
  writer.postMessage(msg);
}

function toPost(msg: ToPost) {
  post.postMessage(msg);
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// ---------------------------------------------------------------- parsing

function stripNzb(name: string): string {
  return name.replace(/\.nzb$/i, '');
}

async function parse(id: string, name: string, bytes: ArrayBuffer) {
  try {
    const engine = await loadEngine(testCa);
    const nzb = JSON.parse(engine.parse_nzb(new Uint8Array(bytes))) as NzbJson;
    parsed.set(id, nzb);
    const taken = new Set<string>();
    const files = nzb.files.map((f, idx) => {
      const unique = uniqueName(f.filename, taken);
      taken.add(unique.toLowerCase());
      f.filename = unique;
      return {
        idx,
        name: unique,
        kind: f.kind,
        bytes: f.bytes,
        segments: f.segments.length,
        blocks: f.blocks,
      };
    });
    const summary: JobSummary = {
      id,
      name: nzb.name?.trim() || nzb.title?.trim() || stripNzb(name),
      password: nzb.password,
      bytes: files.reduce((s, f) => s + f.bytes, 0),
      files,
    };
    ui({ type: 'parsed', id, summary });
  } catch (e) {
    ui({ type: 'parsed', id, error: errorText(e) });
  }
}

// ------------------------------------------------------- provider testing

type Test = {
  id: string;
  host: string;
  port: number;
  readyMs: number | null;
  done: boolean;
  timer: ReturnType<typeof setTimeout>;
};
const tests = new Map<number, Test>();

function testProvider(id: string, p: Provider, relay: string) {
  const connId = nextConnId++;
  const timer = setTimeout(() => endTest(connId, false, "The server didn't answer in time."), TEST_TIMEOUT_MS);
  tests.set(connId, { id, host: p.host, port: p.port, readyMs: null, done: false, timer });
  toNet(0, {
    type: 'open',
    connId,
    url: tunnelUrl(relay, p.host, p.port),
    host: p.host.trim(),
    user: p.username,
    pass: p.password,
  });
}

function endTest(connId: number, ok: boolean, message: string, code?: number) {
  const t = tests.get(connId);
  if (!t || t.done) return;
  t.done = true;
  clearTimeout(t.timer);
  tests.delete(connId);
  toNet(0, { type: 'close', connId });
  ui({ type: 'test-result', id: t.id, ok, message, code });
}

function authMessage(code: number): string {
  if (code === 481 || code === 482) return `Login failed: wrong username or password (${code})`;
  if (code === 480) return `Login failed: the server requires a login (${code})`;
  return `Login failed (${code})`;
}

function closeMessage(m: Extract<FromNet, { type: 'closed' }>, host: string, port: number): string {
  if (m.reason.startsWith('TLS error')) return `Couldn't connect securely to ${host}: ${m.reason.slice(11)}`;
  if (m.wsCode >= 4000 || m.wsCode === 1006 || !m.opened) return describeClose(m.wsCode || 1006, host, port);
  return m.reason ? `The connection closed: ${m.reason}` : describeClose(m.wsCode, host, port);
}

function onTestEvent(m: FromNet, t: Test) {
  switch (m.type) {
    case 'ready':
      t.readyMs = m.ms;
      break;
    case 'auth-ok':
      endTest(m.connId, true, `Connected in ${t.readyMs ?? m.ms} ms. Logged in.`);
      break;
    case 'auth-failed':
      endTest(m.connId, false, authMessage(m.code), m.code);
      break;
    case 'busy':
      endTest(
        m.connId,
        false,
        `Connected, but the server refused more connections (${m.code}). Close other newsreaders or lower the connection count.`,
        m.code,
      );
      break;
    case 'closed':
      endTest(m.connId, false, closeMessage(m, t.host, t.port));
      break;
  }
}

// -------------------------------------------------------------- the job

type ProvRt = {
  cfg: Provider;
  index: number;
  usable: boolean;
  error: string | null;
  target: number;
  /** The configured connection count; target shrinks below it on "busy". */
  max: number;
  /** When the last "busy" reply came, for growing target back. */
  lastBusy: number;
  open: number;
  authedOnce: boolean;
  failures: number;
  busy: number;
  backoffUntil: number;
  retry: number[];
  articles: number;
};

type ConnRt = {
  id: number;
  prov: ProvRt;
  net: number;
  authed: boolean;
  closing: boolean;
  busy: boolean;
  inflight: Set<number>;
  openedAt: number;
  /** Last message from this connection (performance.now()). */
  lastActivity: number;
};

type JobFile = {
  idx: number;
  name: string;
  kind: FileKind;
  blocks: number | null;
  estBytes: number;
  segStart: number;
  segCount: number;
  scheduled: boolean;
  received: number;
  done: number;
  written: number;
  missing: number;
  yencName: string | null;
  fileSize: number | null;
  closeSent: boolean;
  closed: boolean;
};

class Job {
  readonly id: string;
  readonly name: string;
  readonly folder: string;
  readonly relay: string;
  readonly target: Target;
  readonly cleanup: boolean;
  readonly password: string | null;
  readonly provs: ProvRt[];
  readonly files: JobFile[];
  readonly segFile: Uint32Array;
  readonly segMsg: string[];
  readonly segState: Uint8Array;
  readonly segTried: Uint32Array;
  readonly segWritten: Uint8Array;
  readonly conns = new Map<number, ConnRt>();
  phase: Phase = 'connecting';
  detail: string | null = null;
  queue: number[] = [];
  qHead = 0;
  grid: number[] = [];
  cells: number[] = [];
  scheduledSegs = 0;
  settled = 0;
  received = 0;
  written = 0;
  /** fill() held back requests because the disk is behind. */
  throttled = false;
  stalledConns = 0;
  refusedConns = 0;
  backupSegs = 0;
  missingSegs = 0;
  repairedSegs = 0;
  postDone = 0;
  postTotal = 0;
  messages: string[] = [];
  writerError: string | null = null;
  fetchingVolumes = false;
  /** Time spent receiving articles, summed over download phases. */
  transferMs = 0;
  firstByteAt = 0;
  par2Name: string | null = null;
  sliceSize = 0;
  ticker: ReturnType<typeof setInterval>;
  pumpTimer: ReturnType<typeof setTimeout> | null = null;
  finished = false;
  stallMs: number;

  constructor(start: StartJob, nzb: NzbJson) {
    this.id = start.id;
    this.name = start.name;
    this.folder = start.folder;
    this.relay = start.relay;
    this.target = start.target;
    this.cleanup = start.cleanup;
    this.password = nzb.password;
    this.provs = start.providers
      .filter((p) => p.enabled)
      .slice(0, 31)
      .map((cfg, index) => ({
        cfg,
        index,
        usable: true,
        error: null,
        target: Math.max(1, Math.min(50, cfg.connections | 0)),
        max: Math.max(1, Math.min(50, cfg.connections | 0)),
        lastBusy: 0,
        open: 0,
        authedOnce: false,
        failures: 0,
        busy: 0,
        backoffUntil: 0,
        retry: [],
        articles: 0,
      }));
    let total = 0;
    for (const f of nzb.files) total += f.segments.length;
    this.segFile = new Uint32Array(total);
    this.segMsg = new Array<string>(total);
    this.segState = new Uint8Array(total);
    this.segTried = new Uint32Array(total);
    this.segWritten = new Uint8Array(total);
    const selected = new Set(start.selected);
    let s = 0;
    this.files = nzb.files.map((f, idx) => {
      const segStart = s;
      for (const seg of f.segments) {
        this.segFile[s] = idx;
        this.segMsg[s] = seg[2];
        s++;
      }
      return {
        idx,
        name: f.filename,
        kind: f.kind,
        blocks: f.blocks,
        // yEnc adds about 2% plus headers.
        estBytes: Math.round(f.bytes * 0.97),
        segStart,
        segCount: f.segments.length,
        scheduled: false,
        received: 0,
        done: 0,
        written: 0,
        missing: 0,
        yencName: null,
        fileSize: null,
        closeSent: false,
        closed: false,
      };
    });
    const order = downloadOrder(
      this.files.map((f) => ({
        idx: f.idx,
        name: f.name,
        kind: f.kind,
        bytes: f.estBytes,
        selected: selected.has(f.idx),
      })),
    );
    for (const i of order) this.schedule(this.files[i]);
    const index = this.files.find((f) => f.kind === 'par2' && f.scheduled);
    this.par2Name = index?.name ?? null;
    this.stallMs = start.stallMs ?? STALL_MS;
    this.ticker = setInterval(() => {
      this.watchdog();
      this.report();
    }, 250);
  }

  schedule(f: JobFile) {
    f.scheduled = true;
    for (let s = f.segStart; s < f.segStart + f.segCount; s++) {
      this.queue.push(s);
      this.grid.push(s);
    }
    this.scheduledSegs += f.segCount;
  }

  // --- reporting

  setCell(seg: number, state: number) {
    if (this.segState[seg] === state) return;
    this.segState[seg] = state;
    this.cells.push(seg, state);
  }

  sendGrid() {
    const order = Uint32Array.from(this.grid);
    const states = Uint8Array.from(this.grid, (s) => this.segState[s]);
    ui({ type: 'grid', jobId: this.id, order, states }, [order.buffer, states.buffer]);
    this.cells = [];
  }

  report() {
    if (this.cells.length) {
      const updates = Uint32Array.from(this.cells);
      this.cells = [];
      ui({ type: 'cells', jobId: this.id, updates }, [updates.buffer]);
    }
    const files: FileProgress[] = this.files
      .filter((f) => f.scheduled)
      .map((f) => ({
        idx: f.idx,
        name: f.name,
        kind: f.kind,
        bytes: f.fileSize ?? f.estBytes,
        done: f.received,
        segments: f.segCount,
        missing: f.missing,
        state:
          f.done + f.missing < f.segCount
            ? f.done + f.missing > 0
              ? 'active'
              : 'waiting'
            : f.missing > 0
              ? 'incomplete'
              : 'done',
      }));
    const total = files.reduce((s, f) => s + f.bytes, 0);
    const progress: Progress = {
      jobId: this.id,
      phase: this.phase,
      received: this.received,
      written: this.written,
      saving: this.fetching && this.settled >= this.scheduledSegs,
      waitingForDisk: this.throttled,
      stalled: this.stalledConns,
      refused: this.refusedConns,
      total,
      segmentsDone: this.settled - this.missingSegs,
      segmentsTotal: this.scheduledSegs,
      segmentsMissing: this.missingSegs - this.repairedSegs,
      segmentsBackup: this.backupSegs,
      segmentsRepaired: this.repairedSegs,
      files,
      providers: this.provs.map((p) => ({
        id: p.cfg.id,
        name: p.cfg.name || p.cfg.host,
        connections: [...this.conns.values()].filter((c) => c.prov === p && c.authed).length,
        target: p.target,
        inflight: [...this.conns.values()].reduce((n, c) => n + (c.prov === p ? c.inflight.size : 0), 0),
        error: p.error,
        articles: p.articles,
      })),
      detail: this.detail,
      postDone: this.postDone,
      postTotal: this.postTotal,
    };
    ui({ type: 'progress', progress });
  }

  // --- scheduling

  remainingPrimary(): number {
    return this.queue.length - this.qHead;
  }

  take(p: ProvRt): number {
    while (p.retry.length) {
      const s = p.retry.shift()!;
      if (this.segState[s] === Cell.pending) return s;
    }
    if (p.cfg.backup) return -1;
    while (this.qHead < this.queue.length) {
      const s = this.queue[this.qHead++];
      if (this.segState[s] !== Cell.pending) continue;
      if (this.segTried[s] & (1 << p.index)) {
        this.route(s, p);
        continue;
      }
      return s;
    }
    return -1;
  }

  schedulePump(delay: number) {
    if (this.pumpTimer) return;
    this.pumpTimer = setTimeout(() => {
      this.pumpTimer = null;
      this.pump();
    }, delay);
  }

  /** Downloading articles (including recovery files during repair). */
  get fetching(): boolean {
    return this.phase === 'downloading' || this.phase === 'connecting' || (this.phase === 'repairing' && this.fetchingVolumes);
  }

  pump() {
    if (!this.fetching) return;
    const now = Date.now();
    let anyUsable = false;
    for (const p of this.provs) {
      if (!p.usable) continue;
      anyUsable = true;
      const work = p.retry.length + (p.cfg.backup ? 0 : this.remainingPrimary());
      const desired = work > 0 ? Math.min(p.target, Math.max(1, Math.ceil(work / 2))) : 0;
      if (p.open < desired) {
        if (now < p.backoffUntil) {
          this.schedulePump(p.backoffUntil - now + 10);
        } else {
          while (p.open < desired) this.openConn(p);
        }
      }
    }
    if (!anyUsable && this.settled < this.scheduledSegs) {
      const reasons = this.provs.map((p) => `${p.cfg.name || p.cfg.host}: ${p.error ?? 'unavailable'}`);
      this.fail(`No provider could be used. ${reasons.join(' ')}`);
      return;
    }
    for (const c of this.conns.values()) this.fill(c);
  }

  openConn(p: ProvRt) {
    const id = nextConnId++;
    const net = netRound++ % nets.length;
    const now = performance.now();
    const c: ConnRt = {
      id,
      prov: p,
      net,
      authed: false,
      closing: false,
      busy: false,
      inflight: new Set(),
      openedAt: now,
      lastActivity: now,
    };
    this.conns.set(id, c);
    p.open++;
    toNet(net, {
      type: 'open',
      connId: id,
      url: tunnelUrl(this.relay, p.cfg.host, p.cfg.port),
      host: p.cfg.host.trim(),
      user: p.cfg.username,
      pass: p.cfg.password,
    });
  }

  fill(c: ConnRt) {
    if (!c.authed || c.closing) return;
    if (this.received - this.written > MAX_UNWRITTEN) {
      this.throttled = true;
      return;
    }
    const items: [number, number, string][] = [];
    while (c.inflight.size < WINDOW) {
      const s = this.take(c.prov);
      if (s < 0) break;
      c.inflight.add(s);
      this.setCell(s, Cell.active);
      items.push([s, this.segFile[s], this.segMsg[s]]);
    }
    if (items.length) toNet(c.net, { type: 'request', connId: c.id, items });
    else if (c.inflight.size === 0 && this.idleConn(c)) this.closeConn(c);
  }

  /** A connection with nothing to do and no more work coming for it. */
  idleConn(c: ConnRt): boolean {
    return c.prov.retry.length === 0 && (c.prov.cfg.backup || this.remainingPrimary() === 0) && this.settled + this.inflightCount() >= this.scheduledSegs;
  }

  inflightCount(): number {
    let n = 0;
    for (const c of this.conns.values()) n += c.inflight.size;
    return n;
  }

  closeConn(c: ConnRt) {
    if (c.closing) return;
    c.closing = true;
    toNet(c.net, { type: 'close', connId: c.id });
  }

  /**
   * Gives up on a connection without waiting for it to close: a dead socket
   * can take minutes to report it. Its articles are requeued now, and it no
   * longer counts toward the provider's connections.
   */
  abandon(c: ConnRt) {
    this.closeConn(c);
    this.conns.delete(c.id);
    c.prov.open = Math.max(0, c.prov.open - 1);
    this.requeue(c);
  }

  /** Runs with every progress report (4 times a second). */
  watchdog() {
    if (!this.fetching || this.finished) return;
    const now = performance.now();
    let changed = false;
    for (const c of [...this.conns.values()]) {
      if (c.closing) continue;
      const p = c.prov;
      if (!c.authed && now - c.openedAt > CONNECT_MS) {
        this.abandon(c);
        p.failures++;
        p.backoffUntil = Date.now() + Math.min(30000, 1000 * 2 ** p.failures);
        changed = true;
      } else if (c.authed && c.inflight.size > 0 && now - c.lastActivity > this.stallMs) {
        this.abandon(c);
        this.stalledConns++;
        changed = true;
      }
    }
    for (const p of this.provs) {
      if (p.usable && p.target < p.max && Date.now() - p.lastBusy > REGROW_MS) {
        p.target++;
        p.busy = 0;
        p.lastBusy = Date.now();
        changed = true;
      }
    }
    if (changed) this.pump();
  }

  /** Sends a segment to the next provider that hasn't tried it. */
  route(s: number, from: ProvRt | null) {
    if (from) this.segTried[s] |= 1 << from.index;
    const q = nextProvider(this.segTried[s], this.provs);
    if (q >= 0) {
      this.setCell(s, Cell.pending);
      this.provs[q].retry.push(s);
    } else {
      this.settle(s, Cell.missing);
    }
  }

  settle(s: number, state: number) {
    this.setCell(s, state);
    this.settled++;
    const f = this.files[this.segFile[s]];
    if (state === Cell.missing) {
      f.missing++;
      this.missingSegs++;
    } else {
      f.done++;
      if (state === Cell.backup) this.backupSegs++;
    }
    this.maybeCloseFile(f);
  }

  /** Puts a connection's unanswered segments back at the front. */
  requeue(c: ConnRt) {
    const segs = [...c.inflight];
    c.inflight.clear();
    for (const s of segs) {
      if (this.segState[s] !== Cell.active) continue;
      this.setCell(s, Cell.pending);
      if (c.prov.usable) c.prov.retry.unshift(s);
      else this.route(s, c.prov);
    }
  }

  disableProvider(p: ProvRt, error: string) {
    p.usable = false;
    p.error = error;
    for (const c of this.conns.values()) {
      if (c.prov === p) this.closeConn(c);
    }
    const retry = p.retry.splice(0);
    for (const s of retry) this.route(s, p);
  }

  onNet(m: FromNet) {
    const c = this.conns.get(m.connId);
    if (!c) return;
    const p = c.prov;
    c.lastActivity = performance.now();
    switch (m.type) {
      case 'ready':
        break;
      case 'auth-ok':
        c.authed = true;
        p.authedOnce = true;
        p.failures = 0;
        if (this.phase === 'connecting') this.phase = 'downloading';
        this.fill(c);
        break;
      case 'auth-failed':
        this.disableProvider(p, authMessage(m.code));
        break;
      case 'busy':
        c.busy = true;
        p.busy++;
        this.refusedConns++;
        p.lastBusy = Date.now();
        p.target = Math.max(1, Math.min(p.target, p.open - 1));
        p.backoffUntil = Date.now() + Math.min(30000, 2000 * 2 ** Math.min(p.busy, 4));
        break;
      case 'segment': {
        if (!c.inflight.delete(m.segId)) break;
        if (!m.crcOk) {
          this.route(m.segId, p);
          break;
        }
        const f = this.files[m.fileIdx];
        if (!this.firstByteAt) this.firstByteAt = performance.now();
        f.received += m.bytes;
        this.received += m.bytes;
        p.articles++;
        if (m.name && !f.yencName) f.yencName = m.name;
        if (m.fileSize > 0) f.fileSize = m.fileSize;
        this.settle(m.segId, p.cfg.backup ? Cell.backup : Cell.done);
        this.fill(c);
        break;
      }
      case 'missing':
        if (!c.inflight.delete(m.segId)) break;
        this.route(m.segId, p);
        this.fill(c);
        break;
      case 'closed': {
        this.conns.delete(c.id);
        p.open = Math.max(0, p.open - 1);
        this.requeue(c);
        if (!c.closing && p.usable) {
          if (!c.authed && !c.busy) {
            p.failures++;
            const why = closeMessage(m, p.cfg.host, p.cfg.port);
            if ((!p.authedOnce && p.failures >= MAX_FAILURES) || m.wsCode === 4001) {
              this.disableProvider(p, why);
            } else {
              p.backoffUntil = Date.now() + Math.min(30000, 1000 * 2 ** p.failures);
            }
          }
        }
        break;
      }
    }
    this.pump();
    this.checkDownloaded();
  }

  onWriter(m: FromWriter) {
    switch (m.type) {
      case 'written': {
        // Count each article once, even if a replaced connection also wrote it.
        if (this.segWritten[m.segId]) break;
        this.segWritten[m.segId] = 1;
        const f = this.files[m.fileIdx];
        f.written++;
        this.written += m.bytes;
        this.maybeCloseFile(f);
        if (this.throttled && this.received - this.written <= MAX_UNWRITTEN / 2) {
          this.throttled = false;
          for (const c of this.conns.values()) this.fill(c);
        }
        break;
      }
      case 'file-closed': {
        const f = this.files[m.fileIdx];
        f.name = m.name;
        f.closed = true;
        this.checkDownloaded();
        break;
      }
      case 'error':
        this.writerError = m.message;
        this.fail(m.message);
        break;
    }
  }

  maybeCloseFile(f: JobFile) {
    if (f.closeSent || f.done + f.missing < f.segCount || f.written < f.done) return;
    f.closeSent = true;
    let rename: string | null = null;
    // The yEnc header's name wins only over an obfuscated NZB name: many posts
    // obfuscate the yEnc names and keep the real ones in the NZB.
    if (f.yencName && looksObfuscated(f.name) && !looksObfuscated(f.yencName)) {
      const clean = sanitize(f.yencName);
      const taken = new Set(this.files.filter((x) => x !== f).map((x) => x.name.toLowerCase()));
      if (clean && clean !== f.name && !taken.has(clean.toLowerCase())) rename = clean;
    }
    toWriter({ type: 'close-file', fileIdx: f.idx, size: f.fileSize, rename });
  }

  checkDownloaded() {
    if (this.finished || !this.fetching) return;
    if (this.settled < this.scheduledSegs) return;
    if (this.files.some((f) => f.scheduled && !f.closed)) return;
    for (const c of this.conns.values()) this.closeConn(c);
    if (this.firstByteAt) {
      this.transferMs += performance.now() - this.firstByteAt;
      this.firstByteAt = 0;
    }
    if (this.fetchingVolumes) {
      this.fetchingVolumes = false;
      void this.repair();
    } else {
      void this.verify();
    }
  }

  // --- post-processing

  dataNames(): string[] {
    return this.files.filter((f) => f.scheduled && f.kind !== 'par2' && f.kind !== 'par2vol').map((f) => f.name);
  }

  postCall<T extends FromPost['type']>(msg: ToPost, want: T): Promise<Extract<FromPost, { type: T }>> {
    return new Promise((resolve) => {
      postWaiter = (m: FromPost) => {
        if (m.type === 'post-progress') {
          this.postDone = m.done;
          this.postTotal = m.total;
          this.detail = m.detail;
          return;
        }
        if (m.type === want) {
          postWaiter = null;
          resolve(m as Extract<FromPost, { type: T }>);
        }
      };
      toPost(msg);
    });
  }

  async verify() {
    const par2 = this.files.find((f) => f.kind === 'par2' && f.scheduled && f.closed && f.done > 0);
    if (!par2) {
      if (this.missingSegs > 0) {
        this.messages.push(
          `${this.missingSegs} ${this.missingSegs === 1 ? 'article was' : 'articles were'} missing and there is no PAR2 file to repair the damage.`,
        );
      }
      return this.extract();
    }
    this.par2Name = par2.name;
    this.phase = 'verifying';
    this.postDone = 0;
    this.postTotal = 0;
    this.detail = null;
    const r: VerifyResult = await this.postCall(
      {
        type: 'verify',
        target: this.target,
        folder: this.folder,
        par2: par2.name,
        files: this.dataNames(),
        skipped: this.files.filter((f) => !f.scheduled && f.kind !== 'par2' && f.kind !== 'par2vol').map((f) => f.name),
      },
      'verified',
    );
    if (this.finished) return;
    if (r.error) {
      this.messages.push(`Verification skipped: ${r.error}`);
      return this.extract();
    }
    for (const [from, to] of r.renames) {
      const f = this.files.find((x) => x.name === from);
      if (f) f.name = to;
    }
    this.sliceSize = r.sliceSize;
    if (r.ok) return this.extract();
    const volumes = this.files
      .filter((f) => f.kind === 'par2vol' && !f.scheduled)
      .map((f) => ({ idx: f.idx, blocks: volumeBlocks(f.name, f.estBytes, r.sliceSize), bytes: f.estBytes }));
    const pick = pickVolumes(volumes, r.need);
    if (!pick) {
      const have = volumes.reduce((s, v) => s + v.blocks, 0);
      this.messages.push(
        `Repair isn't possible: ${r.need} recovery blocks are needed and only ${have} are available. Damaged: ${r.damagedFiles.join(', ')}.`,
      );
      return this.extract();
    }
    this.fetchingVolumes = true;
    this.phase = 'repairing';
    this.detail = `Downloading ${pick.length} recovery ${pick.length === 1 ? 'file' : 'files'}`;
    for (const v of pick) this.schedule(this.files[v.idx]);
    toWriter({
      type: 'job',
      jobId: this.id,
      target: this.target,
      folder: this.folder,
      files: this.files.map((f) => ({ idx: f.idx, name: f.name })),
    });
    this.sendGrid();
    await writerReady();
    this.pump();
  }

  async repair() {
    this.phase = 'repairing';
    this.detail = null;
    this.postDone = 0;
    this.postTotal = 0;
    const volumes = this.files.filter((f) => f.kind === 'par2vol' && f.scheduled).map((f) => f.name);
    const r = await this.postCall(
      { type: 'repair', target: this.target, folder: this.folder, par2: this.par2Name!, volumes },
      'repaired',
    );
    if (this.finished) return;
    if (r.ok) {
      for (const s of this.grid) {
        if (this.segState[s] === Cell.missing) {
          this.setCell(s, Cell.repaired);
          this.repairedSegs++;
        }
      }
    } else {
      this.messages.push(r.message);
    }
    return this.extract();
  }

  async extract() {
    if (this.finished) return;
    this.phase = 'extracting';
    this.detail = null;
    this.postDone = 0;
    this.postTotal = 0;
    const par2Files = this.files.filter((f) => f.scheduled && (f.kind === 'par2' || f.kind === 'par2vol')).map((f) => f.name);
    const r = await this.postCall(
      {
        type: 'extract',
        target: this.target,
        folder: this.folder,
        files: this.dataNames(),
        password: this.password,
        cleanup: this.cleanup && this.missingSegs === this.repairedSegs,
        par2Files,
      },
      'extracted',
    );
    if (this.finished) return;
    this.messages.push(...r.messages);
    this.complete(r.files, r.ok);
  }

  complete(files: string[], ok: boolean) {
    this.phase = 'done';
    this.detail = null;
    this.stop();
    const result: JobResult = {
      ok: ok && this.missingSegs === this.repairedSegs,
      files,
      messages: this.messages,
      password: this.password,
      missing: this.missingSegs - this.repairedSegs,
      repaired: this.repairedSegs,
      bytes: this.received,
      transferMs: Math.round(this.transferMs),
    };
    ui({ type: 'done', jobId: this.id, result });
  }

  fail(message: string) {
    if (this.finished) return;
    this.phase = 'failed';
    this.detail = message;
    this.stop();
    toWriter({ type: 'abort' });
    ui({ type: 'error', jobId: this.id, message });
  }

  cancel() {
    if (this.finished) return;
    this.phase = 'cancelled';
    this.stop();
    toWriter({ type: 'abort' });
  }

  stop() {
    this.report();
    this.finished = true;
    clearInterval(this.ticker);
    if (this.pumpTimer) clearTimeout(this.pumpTimer);
    for (const c of this.conns.values()) this.closeConn(c);
    if (job === this) job = null;
  }
}

/** Same rules as the engine's sanitize, for yEnc names. */
function sanitize(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? name;
  return base
    .replace(/[<>:"|?*\u0000-\u001f]/g, '_')
    .trim()
    .replace(/^\.+/, '')
    .replace(/[. ]+$/, '')
    .slice(0, 200);
}

let job: Job | null = null;
let postWaiter: ((m: FromPost) => void) | null = null;
let readyWaiter: (() => void) | null = null;

function writerReady(): Promise<void> {
  return new Promise((resolve) => (readyWaiter = resolve));
}

async function start(s: StartJob) {
  const nzb = parsed.get(s.id);
  if (!nzb) {
    ui({ type: 'error', jobId: s.id, message: 'That NZB is no longer loaded. Drop it again.' });
    return;
  }
  if (job) job.cancel();
  const folder = jobFolderName(s.folder || s.name);
  const j = new Job({ ...s, folder }, nzb);
  job = j;
  if (j.provs.length === 0) {
    j.fail('Add a Usenet provider first.');
    return;
  }
  toWriter({
    type: 'job',
    jobId: j.id,
    target: s.target,
    folder,
    files: j.files.map((f) => ({ idx: f.idx, name: f.name })),
  });
  j.sendGrid();
  await writerReady();
  if (j.finished) return;
  j.phase = 'connecting';
  j.pump();
  j.report();
}

function onNet(m: FromNet) {
  const t = tests.get(m.connId);
  if (t) {
    onTestEvent(m, t);
    return;
  }
  job?.onNet(m);
}

self.onmessage = (e: MessageEvent) => {
  const msg = e.data as ToCoordinator | { type: 'wire'; testCa: Uint8Array | null };
  switch (msg.type) {
    case 'wire': {
      const ports = [...e.ports];
      post = ports.pop()!;
      writer = ports.pop()!;
      nets = ports;
      testCa = msg.testCa;
      nets.forEach((p) => (p.onmessage = (m: MessageEvent<FromNet>) => onNet(m.data)));
      writer.onmessage = (m: MessageEvent<FromWriter>) => {
        if (m.data.type === 'ready') {
          readyWaiter?.();
          readyWaiter = null;
          return;
        }
        job?.onWriter(m.data);
      };
      post.onmessage = (m: MessageEvent<FromPost>) => postWaiter?.(m.data);
      break;
    }
    case 'init':
      break;
    case 'parse':
      void parse(msg.id, msg.name, msg.bytes);
      break;
    case 'test-provider':
      testProvider(msg.id, msg.provider, msg.relay);
      break;
    case 'start':
      void start(msg.job);
      break;
    case 'cancel':
      if (job?.id === msg.jobId) job.cancel();
      break;
  }
};
