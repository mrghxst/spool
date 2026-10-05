// App state (Svelte 5 runes). Workers start on first use.

import { Client } from './client';
import * as db from './db';
import { clearOpfs, fileAt, opfsJobsRoot } from './fs';
import { fetchRelayInfo, normalizeRelay, type RelayInfo } from './core/relay';
import type { FromCoordinator } from './workers/protocol';
import {
  Cell,
  type JobResult,
  type JobSummary,
  type Progress,
  type Provider,
  type Settings,
  type Target,
  type Theme,
} from './types';

export const DEFAULT_RELAY = normalizeRelay(import.meta.env.VITE_DEFAULT_RELAY || 'ws://localhost:8080');

const params = new URLSearchParams(location.search);
/** E2E mode: always stage in OPFS (native pickers can't be automated). */
export const E2E = params.has('e2e');

export const canPickFolder = typeof window !== 'undefined' && 'showDirectoryPicker' in window && !E2E;

export function defaultSettings(): Settings {
  return { relay: DEFAULT_RELAY, remember: true, theme: 'system', cleanup: true };
}

export function newProvider(): Provider {
  return {
    id: crypto.randomUUID(),
    name: '',
    host: '',
    port: 563,
    username: '',
    password: '',
    connections: 8,
    backup: false,
    enabled: true,
  };
}

/** Segment grid data, kept out of reactive state for speed. */
export class GridModel {
  order: Uint32Array = new Uint32Array(0);
  states: Uint8Array = new Uint8Array(0);
  version = 0;

  layout(order: Uint32Array, states: Uint8Array) {
    let max = 0;
    for (const s of order) max = Math.max(max, s + 1);
    const next = new Uint8Array(Math.max(max, this.states.length));
    next.set(this.states.subarray(0, Math.min(this.states.length, next.length)));
    order.forEach((s, i) => (next[s] = states[i]));
    this.order = order;
    this.states = next;
    this.version++;
  }

  apply(updates: Uint32Array) {
    for (let i = 0; i + 1 < updates.length; i += 2) {
      const s = updates[i];
      if (s < this.states.length) this.states[s] = updates[i + 1];
    }
    this.version++;
  }

  count(state: number): number {
    let n = 0;
    for (const s of this.order) if (this.states[s] === state) n++;
    return n;
  }
}

export type JobEntry = {
  id: string;
  summary: JobSummary;
  selected: number[];
  status: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  progress: Progress | null;
  result: JobResult | null;
  error: string | null;
  target: Target | null;
  folder: string;
  startedAt: number;
  finishedAt: number;
  grid: GridModel;
  saved: boolean;
};

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function decodeTestCa(): Uint8Array | null {
  if (!E2E) return null;
  const raw = (window as unknown as { __SPOOL_TEST_CA__?: string }).__SPOOL_TEST_CA__;
  if (!raw) return null;
  return Uint8Array.from(atob(raw), (c) => c.charCodeAt(0));
}

class AppState {
  providers = $state<Provider[]>([]);
  settings = $state<Settings>(defaultSettings());
  loaded = $state(false);

  relayStatus = $state<'unknown' | 'checking' | 'ok' | 'error'>('unknown');
  relayInfo = $state<RelayInfo | null>(null);
  relayError = $state<string | null>(null);

  sheet = $state<null | 'providers' | 'settings'>(null);
  editing = $state<Provider | null>(null);

  /** A parsed NZB waiting for "Download". */
  pending = $state<JobSummary | null>(null);
  pendingSelected = $state<number[]>([]);
  jobs = $state<JobEntry[]>([]);
  notice = $state<string | null>(null);
  busy = $state(false);

  private client: Client | null = null;
  private dir: FileSystemDirectoryHandle | null = null;

  get hasProviders(): boolean {
    return this.providers.some((p) => p.enabled);
  }

  get activeJob(): JobEntry | null {
    return this.jobs.find((j) => j.status === 'running') ?? null;
  }

  private getClient(): Client {
    if (!this.client) {
      this.client = new Client(decodeTestCa());
      this.client.listen((m) => this.onCoordinator(m));
    }
    return this.client;
  }

  // ------------------------------------------------------------ persistence

  async load() {
    const remember = (await db.get<boolean>('remember')) ?? true;
    const settings = await db.get<Settings>('settings');
    const providers = await db.get<Provider[]>('providers');
    this.settings = { ...defaultSettings(), ...(settings ?? {}), remember };
    this.providers = providers ?? [];
    this.dir = (await db.get<FileSystemDirectoryHandle>('dir')) ?? null;
    this.applyTheme();
    this.loaded = true;
    // Staged files from an earlier visit can't be resumed.
    if (!E2E) void clearOpfs().catch(() => {});
    void this.checkRelay();
  }

  private async persist() {
    if (!this.settings.remember) return;
    await db.set('remember', true);
    await db.set('settings', $state.snapshot(this.settings));
    await db.set('providers', $state.snapshot(this.providers));
    if (this.dir) await db.set('dir', this.dir);
  }

  save() {
    void this.persist().catch(() => {});
  }

  async setRemember(on: boolean) {
    this.settings.remember = on;
    if (on) {
      this.save();
    } else {
      // Keep everything in memory only; wipe what was stored.
      await db.clear();
    }
  }

  setTheme(theme: Theme) {
    this.settings.theme = theme;
    this.applyTheme();
    this.save();
  }

  applyTheme() {
    const t = this.settings.theme;
    if (t === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = t;
  }

  async forgetEverything() {
    for (const j of this.jobs) if (j.status === 'running') this.cancel(j.id);
    await db.deleteDatabase();
    await clearOpfs().catch(() => {});
    if ('caches' in window) {
      for (const k of await caches.keys()) await caches.delete(k);
    }
    this.providers = [];
    this.settings = defaultSettings();
    this.dir = null;
    this.jobs = [];
    this.pending = null;
    this.applyTheme();
    void this.checkRelay();
  }

  exportJson(): string {
    return JSON.stringify(
      {
        app: 'spool',
        version: 1,
        settings: $state.snapshot(this.settings),
        providers: $state.snapshot(this.providers),
      },
      null,
      2,
    );
  }

  importJson(text: string) {
    const data = JSON.parse(text) as { app?: string; settings?: Partial<Settings>; providers?: Provider[] };
    if (data.app !== 'spool' || !Array.isArray(data.providers)) {
      throw new Error("That file isn't a Spool settings export.");
    }
    const providers = data.providers.map((p) => ({ ...newProvider(), ...p, id: p.id || crypto.randomUUID() }));
    this.providers = providers;
    this.settings = { ...defaultSettings(), ...data.settings, remember: this.settings.remember };
    this.applyTheme();
    this.save();
    void this.checkRelay();
  }

  // ----------------------------------------------------------------- relay

  async checkRelay(): Promise<RelayInfo | null> {
    const relay = this.settings.relay;
    this.relayStatus = 'checking';
    this.relayError = null;
    try {
      const info = await fetchRelayInfo(relay);
      if (relay !== this.settings.relay) return null;
      this.relayInfo = info;
      this.relayStatus = 'ok';
      return info;
    } catch (e) {
      if (relay !== this.settings.relay) return null;
      this.relayInfo = null;
      this.relayStatus = 'error';
      this.relayError = errorText(e);
      return null;
    }
  }

  setRelay(url: string) {
    this.settings.relay = normalizeRelay(url) || DEFAULT_RELAY;
    this.save();
  }

  // ------------------------------------------------------------- providers

  upsertProvider(p: Provider) {
    const i = this.providers.findIndex((x) => x.id === p.id);
    if (i >= 0) this.providers[i] = p;
    else this.providers.push(p);
    this.save();
  }

  removeProvider(id: string) {
    this.providers = this.providers.filter((p) => p.id !== id);
    this.save();
  }

  moveProvider(from: number, to: number) {
    if (from === to || to < 0 || to >= this.providers.length) return;
    const list = [...this.providers];
    const [p] = list.splice(from, 1);
    list.splice(to, 0, p);
    this.providers = list;
    this.save();
  }

  testProvider(p: Provider) {
    return this.getClient().testProvider(p, this.settings.relay);
  }

  // ------------------------------------------------------------------ jobs

  async openNzb(file: File) {
    this.notice = null;
    if (!/\.nzb$/i.test(file.name) && file.type !== 'application/x-nzb') {
      this.notice = `${file.name} isn't an .nzb file.`;
      return;
    }
    this.busy = true;
    try {
      const summary = await this.getClient().parse(file);
      this.pending = summary;
      this.pendingSelected = summary.files.filter((f) => f.kind !== 'par2vol').map((f) => f.idx);
    } catch (e) {
      this.notice = errorText(e);
    } finally {
      this.busy = false;
    }
  }

  discardPending() {
    this.pending = null;
  }

  /** Picks (or re-authorises) the download folder. Must run in a click. */
  private async target(): Promise<Target> {
    if (!canPickFolder) return { kind: 'opfs' };
    if (this.dir) {
      const h = this.dir as FileSystemDirectoryHandle & {
        queryPermission(o: { mode: string }): Promise<PermissionState>;
        requestPermission(o: { mode: string }): Promise<PermissionState>;
      };
      let perm = await h.queryPermission({ mode: 'readwrite' });
      if (perm === 'prompt') perm = await h.requestPermission({ mode: 'readwrite' });
      if (perm === 'granted') return { kind: 'fsa', dir: this.dir };
    }
    const picker = (window as unknown as {
      showDirectoryPicker(o: object): Promise<FileSystemDirectoryHandle>;
    }).showDirectoryPicker;
    this.dir = await picker({ id: 'spool-downloads', mode: 'readwrite', startIn: 'downloads' });
    this.save();
    return { kind: 'fsa', dir: this.dir };
  }

  get folderName(): string | null {
    return canPickFolder ? (this.dir?.name ?? null) : null;
  }

  async chooseFolder() {
    this.dir = null;
    try {
      await this.target();
    } catch {
      // Picker dismissed.
    }
  }

  async download() {
    const summary = this.pending;
    if (!summary) return;
    let target: Target;
    try {
      target = await this.target();
    } catch (e) {
      if ((e as DOMException).name === 'AbortError') return;
      this.notice = `Couldn't open the download folder: ${errorText(e)}`;
      return;
    }
    const entry: JobEntry = {
      id: summary.id,
      summary,
      selected: [...this.pendingSelected],
      status: 'queued',
      progress: null,
      result: null,
      error: null,
      target,
      folder: summary.name,
      startedAt: 0,
      finishedAt: 0,
      grid: new GridModel(),
      saved: false,
    };
    this.pending = null;
    this.jobs = [entry, ...this.jobs];
    this.next();
  }

  private next() {
    if (this.activeJob) return;
    const j = [...this.jobs].reverse().find((x) => x.status === 'queued');
    if (!j || !j.target) return;
    j.status = 'running';
    j.startedAt = Date.now();
    // Plain objects only: Svelte proxies can't be posted to a worker.
    const target: Target = j.target.kind === 'fsa' ? { kind: 'fsa', dir: j.target.dir } : { kind: 'opfs' };
    this.getClient().start({
      id: j.id,
      name: j.summary.name,
      folder: j.folder,
      selected: [...j.selected],
      providers: $state.snapshot(this.providers) as Provider[],
      relay: this.settings.relay,
      target,
      cleanup: this.settings.cleanup,
    });
  }

  cancel(id: string) {
    const j = this.jobs.find((x) => x.id === id);
    if (!j) return;
    if (j.status === 'running') this.getClient().cancel(id);
    j.status = 'cancelled';
    this.next();
  }

  dismiss(id: string) {
    this.jobs = this.jobs.filter((j) => j.id !== id);
  }

  private onCoordinator(m: FromCoordinator) {
    const j = 'jobId' in m ? this.jobs.find((x) => x.id === m.jobId) : undefined;
    switch (m.type) {
      case 'progress': {
        const job = this.jobs.find((x) => x.id === m.progress.jobId);
        if (job && job.status === 'running') job.progress = m.progress;
        break;
      }
      case 'grid':
        j?.grid.layout(m.order, m.states);
        break;
      case 'cells':
        j?.grid.apply(m.updates);
        break;
      case 'done':
        if (j) {
          j.result = m.result;
          j.finishedAt = Date.now();
          j.status = 'done';
          if (j.progress) j.progress = { ...j.progress, phase: 'done' };
        }
        this.next();
        break;
      case 'error':
        if (j) {
          j.error = m.message;
          j.status = 'failed';
        }
        this.next();
        break;
    }
  }

  /** Fallback browsers: download each finished file from OPFS. */
  async saveFiles(id: string) {
    const j = this.jobs.find((x) => x.id === id);
    if (!j?.result) return;
    const root = await opfsJobsRoot();
    const dir = await root.getDirectoryHandle(j.folder);
    for (const path of j.result.files) {
      const file = await (await fileAt(dir, path)).getFile();
      const url = URL.createObjectURL(file);
      const a = document.createElement('a');
      a.href = url;
      a.download = path.split('/').pop()!;
      a.rel = 'noopener';
      document.body.append(a);
      a.click();
      a.remove();
      // Give the browser time to start reading before the next one.
      await new Promise((r) => setTimeout(r, 400));
      setTimeout(() => URL.revokeObjectURL(url), 120_000);
    }
    j.saved = true;
    // Clear the staged copies once the downloads have had time to finish.
    setTimeout(() => void root.removeEntry(j.folder, { recursive: true }).catch(() => {}), 120_000);
  }

  /** Lists the finished files with sizes (Chromium: the chosen folder). */
  async listFiles(id: string): Promise<{ name: string; size: number }[]> {
    const j = this.jobs.find((x) => x.id === id);
    if (!j?.result || !j.target) return [];
    const root = j.target.kind === 'fsa' ? j.target.dir : await opfsJobsRoot();
    const dir = await root.getDirectoryHandle(j.folder);
    const out: { name: string; size: number }[] = [];
    for (const name of j.result.files) {
      try {
        out.push({ name, size: (await (await fileAt(dir, name)).getFile()).size });
      } catch {
        // Removed since.
      }
    }
    return out;
  }
}

export const app = new AppState();
export { Cell };
