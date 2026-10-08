# Guide

Spool downloads files from an `.nzb` straight onto your computer. You need a
Usenet provider account and a browser from 2023 or later.

**[Open Spool](https://spool.itsanon.com)**

## 1. Add a provider

Click **Providers**, then **Add provider**, and fill in what your provider
gave you:

- **Server**, for example `news.example.com`.
- **Port**: `563` for TLS (the default). Port `443` works too where the
  provider offers it. Spool only uses TLS connections.
- **Connections**: how many to open at once. Start with 8 to 20 and stay
  within your plan's limit.
- **Username** and **Password**.

Click **Test connection** to check the login before you save. You can add
several providers. They're used in the order shown: drag them to reorder, or
focus one and press Alt with the arrow keys. Tick **Use only for missing articles** for a block
account or backup: it connects only when another provider is missing an
article.

## 2. Open an .nzb

Drop an `.nzb` file on the page, or click **Choose file**. Spool lists the
files in it. PAR2 recovery volumes are left unticked: Spool fetches only as
many as a repair needs. Click **Download**.

## 3. Choose a folder

In Chrome, Edge, Brave, Opera and Arc, Spool asks for a folder the first time
and writes each download into its own subfolder there. Segments are written
in place, so even very large files never sit in memory.

Firefox and Safari can't write into a folder you choose. Spool keeps the
files in the browser's private storage until the job is done; then click
**Save files** to download them.

## 4. Watch it run

The job shows speed, time left and a grid with one square per article:
downloaded, from a backup provider, missing, or repaired.

After the download, Spool:

1. **Verifies** the files with PAR2, and restores obfuscated file names.
2. **Repairs** damage, downloading only the recovery blocks it needs.
3. **Extracts** RAR (including multi-volume), 7z (including `.7z.001`) and
   zip archives, then deletes the parts and PAR2 files. Turn off **Delete
   archive parts and PAR2 files after a verified extract** in Settings to
   keep everything.

Keep the tab open until the job says **Done**. Spool asks before you leave a
page with a download running.

::: tip Encrypted archives
Spool can't decrypt RAR or encrypted 7z and zip archives. It keeps the
archive files and shows the password from the NZB, if there is one, so you
can extract them with a desktop tool.
:::

## The relay

Browsers can't connect to Usenet servers directly, so Spool sends the
encrypted connection through a relay. By default that's the public relay at
`relay.itsanon.com`. The relay only ever sees encrypted bytes.

For the best speed, run the relay on your own computer: then nothing sits
between you and your provider. See [Self-hosting](./self-hosting.md).

## Settings

- **Relay address**: the relay to use. **Test relay** checks it and lists
  which servers it allows.
- **Download folder**: change the folder Chromium browsers write into.
- **Theme**: system, light or dark.
- **Remember on this device**: on by default. Turn it off on a shared or
  school computer; providers and settings then disappear when you close the
  tab.
- **Export settings** and **Import settings**: move your providers to
  another browser. The export file contains your passwords.
- **Forget everything**: deletes providers, settings, the folder choice and
  any staged files from this browser.

## Browser support

| Browser | Support |
| --- | --- |
| Chrome, Edge, Brave, Opera, Arc | Full: files are written into the folder you choose |
| Firefox, Safari | Downloads work; files stay in browser storage until you click **Save files** |

Spool needs WebAssembly SIMD, module workers and the origin private file
system, which all current browsers have.
