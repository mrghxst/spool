// Shared types for the UI and the workers.

export type Provider = {
  id: string;
  name: string;
  host: string;
  port: number; // default 563
  username: string;
  password: string;
  connections: number; // default 8
  backup: boolean; // "Use only for missing articles"
  enabled: boolean;
};

export type Theme = 'system' | 'light' | 'dark';

export type Settings = {
  relay: string;
  remember: boolean;
  theme: Theme;
  cleanup: boolean;
};

export type FileKind = 'par2' | 'par2vol' | 'archive' | 'other';

/** NZB as returned by the engine's `parse_nzb` (JSON). */
export type NzbJson = {
  title: string | null;
  name: string | null;
  password: string | null;
  files: {
    subject: string;
    filename: string;
    kind: FileKind;
    blocks: number | null;
    bytes: number;
    date: number;
    groups: string[];
    /** [number, bytes, message-id] */
    segments: [number, number, string][];
  }[];
};

export type JobFileSummary = {
  idx: number;
  name: string;
  kind: FileKind;
  bytes: number;
  segments: number;
  blocks: number | null;
};

export type JobSummary = {
  id: string;
  name: string;
  password: string | null;
  bytes: number;
  files: JobFileSummary[];
};

/** Where finished files go. */
export type Target =
  | { kind: 'fsa'; dir: FileSystemDirectoryHandle }
  | { kind: 'opfs' };

export type Phase =
  | 'queued'
  | 'connecting'
  | 'downloading'
  | 'verifying'
  | 'repairing'
  | 'extracting'
  | 'done'
  | 'failed'
  | 'cancelled';

/** Segment cell states, shared by the coordinator and the grid. */
export const Cell = {
  pending: 0,
  active: 1,
  done: 2,
  backup: 3,
  missing: 4,
  repaired: 5,
} as const;
export type CellState = (typeof Cell)[keyof typeof Cell];

export type FileProgress = {
  idx: number;
  name: string;
  kind: FileKind;
  bytes: number;
  done: number; // bytes received
  segments: number;
  missing: number;
  state: 'waiting' | 'active' | 'done' | 'incomplete' | 'skipped';
};

export type ProviderStatus = {
  id: string;
  name: string;
  connections: number;
  /** Connections allowed right now (lower after "too many connections"). */
  target: number;
  /** Articles requested and not yet answered. */
  inflight: number;
  error: string | null;
  articles: number;
};

export type Progress = {
  jobId: string;
  phase: Phase;
  received: number; // decoded bytes
  written: number; // decoded bytes on disk
  /** Every article is in; the last writes and file closes are still running. */
  saving: boolean;
  /** Requests are paused until the disk catches up. */
  waitingForDisk: boolean;
  /** Connections replaced because they stopped answering, this job. */
  stalled: number;
  /** "Too many connections" replies, this job. */
  refused: number;
  total: number; // expected decoded bytes
  segmentsDone: number;
  segmentsTotal: number;
  segmentsMissing: number;
  segmentsBackup: number;
  segmentsRepaired: number;
  files: FileProgress[];
  providers: ProviderStatus[];
  detail: string | null;
  postDone: number;
  postTotal: number;
};

export type JobResult = {
  ok: boolean;
  files: string[];
  messages: string[];
  password: string | null;
  missing: number;
  repaired: number;
  /** Decoded bytes received and the time spent receiving them. */
  bytes: number;
  transferMs: number;
};
