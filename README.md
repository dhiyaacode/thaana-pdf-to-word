# Thaana PDF to Word

A free desktop app for teachers and anyone who works with Dhivehi. It turns **Dhivehi (Thaana) PDFs** into clean text you can paste into Word, or into a ready-made **.docx** file, without the scrambled letters and misplaced fili.

Works on **Windows** and **Mac** (Intel and Apple chip). Everything runs on your own computer. Nothing is uploaded.

> **Beta (v0.3).** Typed PDFs are the strong part. Reading scans, photos and newspapers is experimental and still improving. Handwriting is not supported yet.

## What it does
- **PDFs with typed text (made in Word and similar):** the app takes the letters straight out of the PDF and rebuilds each word in the right order, so copy-and-paste gives real Dhivehi. Tables become Word tables. Numbered lists stay lists.
- **Save as Word:** right-to-left, justified, in Faruma or the font you pick.
- **Scans and photos (experimental):** use *Read text*. Newspaper and magazine pages with real columns are read column by column, right to left.

## Download
Go to the **Releases** page and download the file for your computer:
- **Windows:** the `…x64-setup.exe` installer (about 9 MB). It needs WebView2, which Windows 10 and 11 already have.
- **Mac:** the `.dmg` marked `aarch64` for Apple-chip Macs (M1 and later) or `x64` for Intel Macs. Open it and drag the app to Applications.

The app is not code-signed, so your computer will warn you the first time. This is normal for free apps.
- **Windows:** if you see "Windows protected your PC", click **More info ▸ Run anyway**.
- **Mac:** the first time, go to **System Settings ▸ Privacy & Security** and click **Open Anyway**.

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

## How it is built
The app is a Rust program made with [Tauri](https://tauri.app). It shows its interface in the computer's own web view (WebView2 on Windows, WebKit on Mac), which displays and edits right-to-left Thaana text correctly.

- `crates/thaana-core`: the Rust library that does the work.
  - `pages.rs`: opens PDFs and pictures and draws their pages. PDF pages are drawn with [hayro](https://github.com/LaurenzV/hayro), a PDF renderer written in Rust; pictures (PNG, JPEG, TIFF, BMP, WebP, GIF) are decoded with the [image](https://github.com/image-rs/image) crate.
  - `pdf.rs`, `layout.rs`, `repair.rs`: take the text out of typed PDFs, put letters and vowel signs back in order, rebuild tables, repair words with the word list.
  - `scan.rs`: clean up scanned pages, straighten them, find text lines and newspaper columns.
  - `ocr.rs`: turn what the reader recognised on each line into finished text: remove its usual noise, join lines into paragraphs, repair words.
  - It also builds a command-line tool, `thaana-pdf`.
- `src-tauri`: the app itself. It keeps the pages of open files, and does the dialogs, clipboard, Word export and handwriting labels.
- `renderer`: the interface (HTML and JavaScript): buttons, the page list and the text box. It refers to pages by number; the pages themselves stay in Rust. One step still runs here: [Tesseract](https://github.com/naptha/tesseract.js) recognises the text lines of scans, with the models in `lang/`.

Scan reading uses the app's own model `lang/divx.traineddata` when it is present (it is not part of this source repository). Otherwise it uses the standard Tesseract Dhivehi model `lang/div.traineddata` (from tessdata_fast), which is included. It is less accurate.

## Build it yourself
You need Node.js (LTS) and Rust (https://rustup.rs). Then, in this folder:
- `npm install`
- `npm start` to run the app
- `npm run build` to build it. The Windows installer ends up in `src-tauri/target/release/bundle/nsis/`.

Mac builds are made by GitHub Actions (`.github/workflows/build-app.yml`): run it from the Actions tab, or push a `v…` tag. It builds Windows and both kinds of Mac and puts them on a draft release.

## Tests
- `npm test`: the Rust library's tests, including checks against the test PDFs and pictures (paragraphs, vowel signs, bordered and borderless tables, scanned pages, form XObjects, drawing pages, finding text lines).
- `npm run test:app`: builds the real app with test hooks, drops the test PDFs on it, saves a Word file and runs *Read text*.
- `npm run bench:ocr -- some-typed.pdf`: measures how well and how fast scans are read. It makes the app read a typed PDF as pictures and scores the result against the PDF's true text. Run it before and after any change to scan reading (it needs the test build that `npm run test:app` makes). The true text comes from the library's command-line tool: `thaana-pdf --json file.pdf`.

The test PDFs are made on your computer, not stored in the repository. On Windows with Word and Edge installed, run `pwsh test/make-fixtures.ps1`, then `python test/make-fixtures.py` (needs `pip install pillow pypdf`).

## Privacy
No file, text or image leaves your computer. This repository must never contain PDFs, student scripts, exam papers or fonts (see `.gitignore`).

## Licence
Apache License 2.0. See `LICENSE` and `NOTICE`.
