---
layout: home
title: Spool
titleTemplate: Usenet in a browser tab

hero:
  name: spool
  text: Usenet in a browser tab.
  tagline: Drop in an .nzb and get the finished files. Downloading, yEnc decoding, PAR2 repair and extraction all run in the tab, with nothing to install.
  actions:
    - theme: brand
      text: Open Spool
      link: https://spool.itsanon.com
    - theme: alt
      text: Read the guide
      link: /guide
    - theme: alt
      text: GitHub
      link: https://github.com/mrghxst/spool

features:
  - title: Nothing to install
    details: A static web page. Works on locked-down laptops, including school ones. Installable as an app that opens .nzb files.
  - title: End-to-end TLS
    details: TLS runs inside the browser and ends at your provider. The relay only forwards encrypted bytes and can't read your password or what you download.
  - title: Repair and extract
    details: PAR2 verification and repair with only the recovery files it needs, then RAR, 7z and zip extraction and cleanup.
  - title: Several providers
    details: Providers in priority order. Missing articles are retried on the next one; backups connect only when needed.
  - title: Straight to your folder
    details: In Chromium browsers, segments are written in place into the folder you choose, so large files never sit in memory.
  - title: No accounts, no tracking
    details: No analytics or third-party requests. Settings stay in your browser, and "Remember on this device" can be turned off.
---

<div class="hero-shot">
  <img class="light-only" src="./assets/hero-light.png" alt="A download in progress: speed, time left, and a grid with one square per article." />
  <img class="dark-only" src="./assets/hero-dark.png" alt="A download in progress: speed, time left, and a grid with one square per article." />
</div>
