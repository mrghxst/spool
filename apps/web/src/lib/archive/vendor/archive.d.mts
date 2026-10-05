// Types for the Emscripten module built by scripts/build-libarchive.sh.

export type ArchiveModule = {
  _malloc(size: number): number;
  _free(ptr: number): void;
  _spool_open(paths: number, count: number, password: number): number;
  _spool_close(): void;
  _spool_next(): number;
  _spool_entry_path(): number;
  _spool_entry_size(): number;
  _spool_entry_type(): number;
  _spool_entry_encrypted(): number;
  _spool_read(buf: number, size: number): number;
  _spool_skip(): number;
  _spool_error(): number;
  _spool_format(): number;
  HEAPU8: Uint8Array;
  UTF8ToString(ptr: number): string;
  stringToNewUTF8(s: string): number;
  FS: {
    mkdir(path: string): void;
    rmdir(path: string): void;
    mount(type: unknown, opts: { files?: File[]; blobs?: { name: string; data: Blob }[] }, mountpoint: string): void;
    unmount(mountpoint: string): void;
    filesystems: { WORKERFS: unknown };
  };
};

export default function createArchiveModule(opts?: {
  locateFile?: (path: string, prefix: string) => string;
  print?: (s: string) => void;
  printErr?: (s: string) => void;
}): Promise<ArchiveModule>;
