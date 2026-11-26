# Thaana PDF to Word

A free desktop app for teachers and anyone who works with Dhivehi. It turns **Dhivehi (Thaana) PDFs** into clean text you can paste into Word, or into a ready-made **.docx** file, without the scrambled letters and misplaced fili.

Works on **Windows** and **Mac** (Intel and Apple chip). Everything runs on your own computer. Nothing is uploaded.

> **Beta (v0.3).** Typed PDFs are the strong part. Reading scans, photos and newspapers is experimental and still improving. Handwriting is not supported yet.

## What it does
- **PDFs with typed text (made in Word and similar):** the app takes the letters straight out of the PDF and rebuilds each word in the right order, so copy-and-paste gives real Dhivehi. Tables become Word tables. Numbered lists stay lists.
- **Save as Word:** right-to-left, justified, in Faruma or the font you pick.
- **Scans and photos (experimental):** use *Read text*. Newspaper and magazine pages are split into columns and read right to left.

## Download
Go to the **Releases** page and download the `.zip` for your computer:
- **Windows:** `Thaana-OCR-…-Windows.zip`. Unzip it and double-click the `.exe` inside (no install).
- **Mac:** `Thaana-OCR-…-Mac-arm64.zip` for Apple-chip Macs (M1 and later) or `…-Mac-x64.zip` for Intel Macs. Unzip it and drag the app to Applications.

The app is not code-signed, so your computer will warn you the first time. This is normal for free apps.
- **Windows:** if you see "Windows protected your PC", click **More info ▸ Run anyway**.
- **Mac:** open the .dmg, drag the app to Applications. The first time, go to **System Settings ▸ Privacy & Security** and click **Open Anyway**.

## Using it
See the **User Manual** in the `docs` folder (`docs/Thaana-PDF-to-Word-User-Manual.pdf`). In short:
1. **Open scans / PDFs…** (or drag files in).
2. For scans, click **Read text**. Typed PDFs appear straight away.
3. Check the text, then **Copy all** or **Save as Word**.

## Known limits
- Very old PDFs whose text was stored in legacy (non-Unicode) fonts can't be read directly and fall back to the weaker scan reader.
- Tables with borders are rebuilt cell by cell. In tables without borders, a cell that wraps onto a second line is joined back only when the first (right-hand) column of that row does not wrap too.
- With *Ignore Arabic text* on, lines that are mostly Arabic (verses, prayers) are left out. Arabic words inside a Dhivehi sentence are kept, without their vowel marks.
- Scan reading with the standard Dhivehi model gets about 93% of characters and under half of the words exactly right on a clean page (`npm run bench:ocr`). A better model is the way to improve on that.
- Always read through the result before using it for anything important.

## The new Rust version (Tauri)
The app is moving from Electron to [Tauri](https://tauri.app): a Rust program that shows the same interface in the computer's own web view (WebView2 on Windows, WebKit on Mac). It works the same way, reads scans just as fast and gives the same results, but the download is much smaller: the Windows installer is about 9 MB instead of about 120 MB.

- Files, dialogs, the clipboard, Word export and handwriting labels are handled in Rust (`src-tauri/src/`).
- The interface (`renderer/`) is shared with the Electron version. `renderer/bridge.js` connects it to Rust.
- Text is taken out of typed PDFs in Rust (`crates/thaana-core`): a port of the JavaScript reader that gives exactly the same result (`npm run test:parity` compares the two on every page) and is about 3 times faster. If it ever fails on a file, the app falls back to the JavaScript reader.
- Scans are cleaned up and cut into text lines in Rust too (`crates/thaana-core/src/scan.rs`), while the previous page is being read, then read by the same Tesseract engine and models running inside the web view. A 14-page copier scan takes about 19 seconds instead of 33.

To build it you need Node.js (LTS) and Rust (https://rustup.rs). Then run:
- `npm install`
- `npm run app:dev` to run it
- `npm run app:build` to build it. The Windows installer ends up in `src-tauri/target/release/bundle/nsis/`.
- `npm run bench:ocr -- some-typed.pdf` to measure how well and how fast scans are read: it makes the app read a typed PDF as pictures and scores the result against the PDF's true text. Run it before and after any change to scan reading.
- `npm run test:tauri` to test the real app end to end, `npm run test:core` for the Rust reader's own tests, and `npm run test:parity` to compare the Rust and JavaScript readers.

Mac builds are made by GitHub Actions (`.github/workflows/build-app.yml`, run it from the Actions tab or push a `v…` tag). The app is not code-signed, so the first-launch steps above still apply.

## Build it yourself (Electron version)
Install Node.js (LTS), then in this folder run `npm install`, then:
- Windows build: `npm run dist:win`
- Mac build: double-click `build-mac.command`, or `npm run dist:mac-separate`

Scan reading uses the app's own model `lang/divx.traineddata` when it is present (it is not part of this source repository). Otherwise it uses the standard Tesseract Dhivehi model `lang/div.traineddata` (from tessdata_fast), which is included. It is less accurate.

## Tests
- `npm test`: checks the PDF reader against test PDFs (paragraphs, vowel signs, bordered and borderless tables, scanned pages, form XObjects).
- `npm run test:app`: opens the real app, drops the test PDFs on it, saves a Word file and runs *Read text*.

The test PDFs are made on your computer, not stored in the repository. On Windows with Word and Edge installed, run `pwsh test/make-fixtures.ps1`, then `python test/make-fixtures.py` (needs `pip install pillow pypdf`).

## Privacy
No file, text or image leaves your computer. This repository must never contain PDFs, student scripts, exam papers or fonts (see `.gitignore`).

## Licence
Apache License 2.0. See `LICENSE` and `NOTICE`.
