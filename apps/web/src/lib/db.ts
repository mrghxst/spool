// A tiny IndexedDB key-value store. Holds providers, settings and the
// download folder handle, and only when "Remember on this device" is on.

const DB = 'spool';
const STORE = 'kv';

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await openDb();
  try {
    return await new Promise<T>((resolve, reject) => {
      const t = db.transaction(STORE, mode);
      const req = f(t.objectStore(STORE));
      t.oncomplete = () => resolve(req.result);
      t.onerror = () => reject(t.error);
      t.onabort = () => reject(t.error);
    });
  } finally {
    db.close();
  }
}

export async function get<T>(key: string): Promise<T | undefined> {
  try {
    return (await tx('readonly', (s) => s.get(key))) as T | undefined;
  } catch {
    return undefined;
  }
}

export async function set(key: string, value: unknown): Promise<void> {
  await tx('readwrite', (s) => s.put(value, key));
}

export async function del(key: string): Promise<void> {
  await tx('readwrite', (s) => s.delete(key));
}

export async function clear(): Promise<void> {
  try {
    await tx('readwrite', (s) => s.clear());
  } catch {
    // Nothing stored.
  }
}

export function deleteDatabase(): Promise<void> {
  return new Promise((resolve) => {
    const req = indexedDB.deleteDatabase(DB);
    req.onsuccess = req.onerror = req.onblocked = () => resolve();
  });
}
