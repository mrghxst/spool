// Messages between the UI thread and the workers.

import type {
  JobResult,
  JobSummary,
  Progress,
  Provider,
  Target,
} from '../types';

// --- UI <-> coordinator ---

export type StartJob = {
  id: string;
  name: string;
  folder: string;
  selected: number[];
  providers: Provider[];
  relay: string;
  target: Target;
  cleanup: boolean;
};

export type ToCoordinator =
  | { type: 'init'; netCount: number }
  | { type: 'parse'; id: string; name: string; bytes: ArrayBuffer }
  | { type: 'test-provider'; id: string; provider: Provider; relay: string }
  | { type: 'start'; job: StartJob }
  | { type: 'cancel'; jobId: string };

export type FromCoordinator =
  | { type: 'parsed'; id: string; summary?: JobSummary; error?: string }
  | { type: 'test-result'; id: string; ok: boolean; message: string; code?: number }
  | { type: 'progress'; progress: Progress }
  /** Grid layout: the segment ids shown, in order. */
  | { type: 'grid'; jobId: string; order: Uint32Array; states: Uint8Array }
  /** Grid changes: pairs of [segId, state]. */
  | { type: 'cells'; jobId: string; updates: Uint32Array }
  | { type: 'done'; jobId: string; result: JobResult }
  | { type: 'error'; jobId: string; message: string };

// --- coordinator <-> net worker ---

export type OpenConn = {
  type: 'open';
  connId: number;
  url: string;
  host: string;
  user: string;
  pass: string;
};

export type ToNet =
  | OpenConn
  /** [segId, fileIdx, message-id] triples. */
  | { type: 'request'; connId: number; items: [number, number, string][] }
  | { type: 'close'; connId: number };

export type FromNet =
  | { type: 'ready'; connId: number; ms: number }
  | { type: 'auth-ok'; connId: number; ms: number }
  | { type: 'auth-failed'; connId: number; code: number }
  | { type: 'busy'; connId: number; code: number }
  | { type: 'closed'; connId: number; reason: string; wsCode: number; opened: boolean }
  | {
      type: 'segment';
      connId: number;
      segId: number;
      fileIdx: number;
      bytes: number;
      begin: number;
      crcOk: boolean;
      name: string;
      fileSize: number;
    }
  | { type: 'missing'; connId: number; segId: number };

// --- net -> writer, coordinator <-> writer ---

export type ToWriter =
  | { type: 'job'; jobId: string; target: Target; folder: string; files: { idx: number; name: string }[] }
  | { type: 'rename-pending'; fileIdx: number; name: string }
  | { type: 'write'; fileIdx: number; segId: number; offset: number; data: ArrayBuffer }
  | { type: 'close-file'; fileIdx: number; size: number | null; rename: string | null }
  | { type: 'flush' }
  | { type: 'abort' };

export type FromWriter =
  | { type: 'ready' }
  | { type: 'written'; segId: number; fileIdx: number; bytes: number }
  | { type: 'file-closed'; fileIdx: number; name: string }
  | { type: 'flushed' }
  | { type: 'error'; message: string };

// --- coordinator <-> post worker ---

export type PostFile = { name: string; kind: string };

export type ToPost =
  | { type: 'verify'; target: Target; folder: string; par2: string; files: string[]; skipped: string[] }
  | { type: 'repair'; target: Target; folder: string; par2: string; volumes: string[] }
  | {
      type: 'extract';
      target: Target;
      folder: string;
      files: string[];
      password: string | null;
      cleanup: boolean;
      par2Files: string[];
    };

export type VerifyResult = {
  type: 'verified';
  ok: boolean;
  /** Blocks needed to repair. */
  need: number;
  sliceSize: number;
  /** old name -> real name (deobfuscation). */
  renames: [string, string][];
  /** Real names of files in the PAR2 set. */
  names: string[];
  damagedFiles: string[];
  error?: string;
};

export type FromPost =
  | VerifyResult
  | { type: 'repaired'; ok: boolean; message: string; repairedSlices: number }
  | {
      type: 'extracted';
      ok: boolean;
      files: string[];
      messages: string[];
      encrypted: boolean;
      removed: string[];
    }
  | { type: 'post-progress'; phase: 'verifying' | 'repairing' | 'extracting'; done: number; total: number; detail: string };
