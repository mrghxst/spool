// Archive extraction. Replaced by the libarchive implementation in a later
// step; for now it reports archive sets without touching them.

import { archiveSets } from '../core/plan';
import type { Target } from '../types';

export type ExtractResult = {
  ok: boolean;
  extractedSets: number;
  /** Archive parts that were fully extracted (safe to delete). */
  consumed: string[];
  messages: string[];
  encrypted: boolean;
};

export async function extractAll(
  _dir: FileSystemDirectoryHandle,
  _kind: Target['kind'],
  names: string[],
  _password: string | null,
  _onProgress: (done: number, total: number, detail: string) => void,
): Promise<ExtractResult> {
  const sets = archiveSets(names);
  return {
    ok: true,
    extractedSets: 0,
    consumed: [],
    messages: sets.length ? ['Archives were kept as downloaded.'] : [],
    encrypted: false,
  };
}
