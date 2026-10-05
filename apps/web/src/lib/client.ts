// Starts the workers, wires their message ports together and exposes a
// small promise-based API to the UI.

import type { FromCoordinator, StartJob, ToCoordinator } from './workers/protocol';
import type { JobSummary, Provider } from './types';

export type CoordinatorListener = (msg: FromCoordinator) => void;

export class Client {
  private coordinator: Worker;
  private workers: Worker[] = [];
  private listeners = new Set<CoordinatorListener>();
  private waiting = new Map<string, (msg: FromCoordinator) => void>();

  constructor(testCa: Uint8Array | null) {
    const k = Math.max(1, Math.min(navigator.hardwareConcurrency || 2, 4));
    this.coordinator = new Worker(new URL('./workers/coordinator.worker.ts', import.meta.url), {
      type: 'module',
      name: 'coordinator',
    });
    const writer = new Worker(new URL('./workers/writer.worker.ts', import.meta.url), {
      type: 'module',
      name: 'writer',
    });
    const post = new Worker(new URL('./workers/post.worker.ts', import.meta.url), {
      type: 'module',
      name: 'post',
    });
    const coordToNet: MessagePort[] = [];
    const writerFromNet: MessagePort[] = [];
    for (let i = 0; i < k; i++) {
      const net = new Worker(new URL('./workers/net.worker.ts', import.meta.url), {
        type: 'module',
        name: `net-${i}`,
      });
      const toCoord = new MessageChannel();
      const toWriter = new MessageChannel();
      net.postMessage({ type: 'wire', testCa }, [toCoord.port2, toWriter.port2]);
      coordToNet.push(toCoord.port1);
      writerFromNet.push(toWriter.port1);
      this.workers.push(net);
    }
    const coordWriter = new MessageChannel();
    const coordPost = new MessageChannel();
    writer.postMessage({ type: 'wire' }, [coordWriter.port2, ...writerFromNet]);
    post.postMessage({ type: 'wire', testCa }, [coordPost.port2]);
    this.coordinator.postMessage({ type: 'wire', testCa }, [
      ...coordToNet,
      coordWriter.port1,
      coordPost.port1,
    ]);
    this.workers.push(writer, post);
    this.coordinator.onmessage = (e: MessageEvent<FromCoordinator>) => {
      const msg = e.data;
      if ((msg.type === 'parsed' || msg.type === 'test-result') && this.waiting.has(msg.id)) {
        this.waiting.get(msg.id)!(msg);
        this.waiting.delete(msg.id);
        return;
      }
      for (const l of this.listeners) l(msg);
    };
  }

  private send(msg: ToCoordinator, transfer: Transferable[] = []) {
    this.coordinator.postMessage(msg, transfer);
  }

  private call(id: string, msg: ToCoordinator, transfer: Transferable[] = []): Promise<FromCoordinator> {
    return new Promise((resolve) => {
      this.waiting.set(id, resolve);
      this.send(msg, transfer);
    });
  }

  listen(l: CoordinatorListener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  async parse(file: File): Promise<JobSummary> {
    const id = crypto.randomUUID();
    const bytes = await file.arrayBuffer();
    const r = await this.call(id, { type: 'parse', id, name: file.name, bytes }, [bytes]);
    if (r.type !== 'parsed' || !r.summary) {
      throw new Error(r.type === 'parsed' && r.error ? r.error : "Couldn't read that NZB file.");
    }
    return r.summary;
  }

  async testProvider(provider: Provider, relay: string): Promise<{ ok: boolean; message: string }> {
    const id = crypto.randomUUID();
    const r = await this.call(id, { type: 'test-provider', id, provider: $plain(provider), relay });
    if (r.type !== 'test-result') return { ok: false, message: 'Unexpected reply.' };
    return { ok: r.ok, message: r.message };
  }

  start(job: StartJob) {
    this.send({ type: 'start', job: { ...job, providers: job.providers.map($plain) } });
  }

  cancel(jobId: string) {
    this.send({ type: 'cancel', jobId });
  }
}

/** Strips Svelte proxies so objects can be structured-cloned. */
function $plain<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}
