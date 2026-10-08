// File-name helpers.

/**
 * A random-looking file name: no usual extension, or a stem of 20 or more
 * letters and digits with no separators that mixes case or has digits.
 */
export function looksObfuscated(name: string): boolean {
  const base = name.split(/[/\\]/).pop() ?? name;
  const dot = base.lastIndexOf('.');
  const ext = dot > 0 ? base.slice(dot + 1).toLowerCase() : '';
  const stem = dot > 0 ? base.slice(0, dot) : base;
  if (!/^(r\d\d|\d{3}|[a-z][a-z0-9]{1,4})$/.test(ext)) return true;
  return /^[A-Za-z0-9]{20,}$/.test(stem) && /[0-9]|[a-z][^a-z]*[A-Z]|[A-Z][^A-Z]*[a-z]/.test(stem);
}
