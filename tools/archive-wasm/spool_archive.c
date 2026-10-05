// A small pull API over libarchive for Spool's post worker.
//
// JavaScript mounts the archive volumes with Emscripten's WORKERFS (reads go
// straight to disk-backed File objects), opens them with spool_open, then
// loops spool_next / spool_read and writes the data out asynchronously
// between calls. libarchive handles multi-volume RAR and RAR5; for split
// files such as name.7z.001, opening every part with
// archive_read_open_filenames presents them as one seekable stream.

#include <stdlib.h>
#include <string.h>

#include <archive.h>
#include <archive_entry.h>
#include <emscripten/emscripten.h>

static struct archive *A = NULL;
static struct archive_entry *E = NULL;

EMSCRIPTEN_KEEPALIVE
void spool_close(void) {
  if (A) {
    archive_read_free(A);
    A = NULL;
    E = NULL;
  }
}

// Opens `count` volumes (in order). Returns 0 on success.
EMSCRIPTEN_KEEPALIVE
int spool_open(const char **paths, int count, const char *password) {
  spool_close();
  A = archive_read_new();
  if (!A) return -1;
  archive_read_support_filter_all(A);
  archive_read_support_format_rar(A);
  archive_read_support_format_rar5(A);
  archive_read_support_format_7zip(A);
  archive_read_support_format_zip(A);
  archive_read_support_format_tar(A);
  if (password && password[0]) archive_read_add_passphrase(A, password);

  const char **list = malloc(sizeof(char *) * (size_t)(count + 1));
  if (!list) return -1;
  for (int i = 0; i < count; i++) list[i] = paths[i];
  list[count] = NULL;
  int r = archive_read_open_filenames(A, list, 1 << 20);
  free(list);
  return r == ARCHIVE_OK || r == ARCHIVE_WARN ? 0 : r;
}

// 1 = an entry is ready, 0 = end of archive, negative = error.
EMSCRIPTEN_KEEPALIVE
int spool_next(void) {
  if (!A) return -1;
  int r = archive_read_next_header(A, &E);
  if (r == ARCHIVE_EOF) return 0;
  if (r == ARCHIVE_OK || r == ARCHIVE_WARN) return 1;
  return r < 0 ? r : -1;
}

EMSCRIPTEN_KEEPALIVE
const char *spool_entry_path(void) {
  if (!E) return "";
  const char *p = archive_entry_pathname_utf8(E);
  if (!p) p = archive_entry_pathname(E);
  return p ? p : "";
}

// Uncompressed size, or -1 when unknown.
EMSCRIPTEN_KEEPALIVE
double spool_entry_size(void) {
  if (!E || !archive_entry_size_is_set(E)) return -1;
  return (double)archive_entry_size(E);
}

// 1 = regular file, 2 = directory, 0 = anything else (links, devices).
EMSCRIPTEN_KEEPALIVE
int spool_entry_type(void) {
  if (!E) return 0;
  switch (archive_entry_filetype(E)) {
    case AE_IFREG:
      return 1;
    case AE_IFDIR:
      return 2;
    default:
      return 0;
  }
}

EMSCRIPTEN_KEEPALIVE
int spool_entry_encrypted(void) {
  return E ? archive_entry_is_encrypted(E) : 0;
}

// Reads up to `size` bytes of the current entry. 0 = end, negative = error.
EMSCRIPTEN_KEEPALIVE
int spool_read(void *buf, int size) {
  if (!A) return -1;
  la_ssize_t n = archive_read_data(A, buf, (size_t)size);
  return (int)n;
}

EMSCRIPTEN_KEEPALIVE
int spool_skip(void) {
  return A ? archive_read_data_skip(A) : -1;
}

EMSCRIPTEN_KEEPALIVE
const char *spool_error(void) {
  const char *e = A ? archive_error_string(A) : NULL;
  return e ? e : "";
}

EMSCRIPTEN_KEEPALIVE
const char *spool_format(void) {
  const char *f = A ? archive_format_name(A) : NULL;
  return f ? f : "";
}
