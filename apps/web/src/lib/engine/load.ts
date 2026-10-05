// Loads the Rust/WASM engine. Only workers import this, so the engine is
// fetched on first use and never weighs on the initial page load.

import init, * as engine from './pkg/engine.js';
import wasmUrl from './pkg/engine_bg.wasm?url';

export type Engine = typeof engine;

let ready: Promise<Engine> | null = null;

export function loadEngine(testCa?: Uint8Array | null): Promise<Engine> {
  ready ??= init({ module_or_path: wasmUrl }).then(() => {
    // Only E2E builds (cargo feature `test-ca`) export add_test_root.
    const hook = Reflect.get(engine, 'add_test_root') as ((der: Uint8Array) => void) | undefined;
    if (testCa && hook) hook(testCa);
    return engine;
  });
  return ready;
}
