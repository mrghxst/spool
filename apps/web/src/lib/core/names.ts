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

/** Letters and digits only, lowercased: "Some_Name-2021" ~ "some.name.2021". */
function loose(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/** Same name apart from punctuation, so the archive's top folder is redundant. */
export function sameFolder(entry: string, folder: string): boolean {
  const a = loose(entry);
  const b = loose(folder);
  return a.length > 0 && (a === b || (b.length >= 20 && a.startsWith(b)) || (a.length >= 20 && b.startsWith(a)));
}

/** `name` with its stem cut so the whole name is at most `max` characters. */
export function shortName(name: string, max = 60): string {
  if (name.length <= max) return name;
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 && name.length - dot <= 10 ? name.slice(dot) : '';
  return `${name.slice(0, max - ext.length - 1).replace(/[. ]+$/, '')}~${ext}`;
}
