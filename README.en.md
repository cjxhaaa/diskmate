<div align="center">

<img src="app-icon.svg" width="96" alt="DiskMate">

# DiskMate

**See where your C: drive went — and free it up safely**

A disk analyzer and cleaner for Windows · free & open source · 2.6 MB installer · no background services

[![Release](https://img.shields.io/github/v/release/cjxhaaa/diskmate?color=5b6cff)](https://github.com/cjxhaaa/diskmate/releases/latest)
[![License](https://img.shields.io/github/license/cjxhaaa/diskmate?color=8a55e8)](LICENSE)
![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078d4)
![Built with Tauri](https://img.shields.io/badge/Tauri%202-Rust%20%2B%20React-24c8db)

[Download](https://github.com/cjxhaaa/diskmate/releases/latest) · [中文说明](README.md)

<img src="docs/screenshots/overview-light.webp" width="880" alt="DiskMate overview">

</div>

> The interface is in Simplified Chinese for now; an English UI is on the roadmap.

## Why

Most cleaners either only empty temp folders, or "boost" your PC without telling you what they deleted — and then stay resident to show ads. DiskMate does the opposite:

- **Accounts for every gigabyte first** — by category, folder and program, measuring what files *actually* occupy (OneDrive placeholders, sparse/compressed files and hard links are handled).
- **Lets you decide** — every item says what it is and what deleting it costs, rated safe / careful / confirm; only safe items are pre-selected.
- **Stays out of the way** — no ads, no services, no autostart; background work runs at the lowest Windows priority on a few threads.

It is especially useful for developers: Gradle and Android SDK/emulator data, `node_modules`, Rust `target`, package manager caches, idle Git worktrees and ever-growing WSL/Docker `.vhdx` disks are often the real culprits.

## Features

- **Overview** — usage by category with explanations.
- **Smart cleanup** — system junk, developer caches, browser caches, old IDE data, idle worktrees; reports the space actually freed.
- **Apps** — program size *plus* the data each app keeps in AppData, last used date; runs the app's own uninstaller, then offers to remove leftovers.
- **Space map & history** — drill-down treemap; compare scans to see what has been growing.
- **File tidy-up** — installers you already installed, archives you already extracted, content-verified duplicates (BLAKE3), large files. Deletes go to the Recycle Bin by default.
- **Move to another drive** — moves big folders (chat files, SDKs, caches) to D: and leaves a junction behind, so apps keep working unchanged; copies, verifies, then deletes, with automatic rollback. Also compacts WSL/Docker virtual disks.
- **Startup apps** — every startup entry with a "fine to turn off / better kept" hint; uses the same switch as Task Manager.
- **Performance** — a Task-Manager-style live view of CPU (per core), memory, disks, network and GPU, plus the programs using them.
- **Window finder** — click any window on screen to see which program owns it, where its file is and its command line.

## Install

Windows 10 1809+ / 11, x64:

- **Installer** — `DiskMate_x.y.z_x64-setup.exe` from [Releases](https://github.com/cjxhaaa/diskmate/releases/latest); installs per user, no admin rights.
- **Portable** — `DiskMate_x.y.z_x64_portable.zip`; unzip and run `DiskMate.exe` (needs the WebView2 runtime, built into Windows 11).
- **Scoop** — `scoop bucket add diskmate https://github.com/cjxhaaa/diskmate`, then `scoop install diskmate`.

The installer is not code-signed yet, so SmartScreen may warn: choose *More info → Run anyway*, or build it yourself.

## Build from source

Requires Node.js 20+, stable Rust and the Visual Studio C++ build tools.

```bash
git clone https://github.com/cjxhaaa/diskmate.git
cd diskmate
npm install
npm run tauri build   # installer in src-tauri/target/release/bundle/nsis/
```

`npm run dev` serves the UI alone at http://127.0.0.1:1430 with sample data, no Rust needed. Implementation notes (in Chinese): [docs/TECHNICAL.md](docs/TECHNICAL.md).

## License

[MIT](LICENSE)
