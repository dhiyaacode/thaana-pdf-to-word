# Thaana PDF to Word

A free desktop app for teachers and anyone who works with Dhivehi. It turns **Dhivehi (Thaana) PDFs** into clean text you can paste into Word, or into a ready-made **.docx** file, without the scrambled letters and misplaced fili.

Works on **Windows** and **Mac** (Intel and Apple chip). Everything runs on your own computer. Nothing is uploaded.

> **Beta (v0.3).** Typed PDFs are the strong part. Reading scans, photos and newspapers is experimental and still improving. Handwriting is not supported yet.

## What it does
- **PDFs with typed text (made in Word and similar):** the app takes the letters straight out of the PDF and rebuilds each word in the right order, so copy-and-paste gives real Dhivehi. Tables become Word tables. Numbered lists stay lists.
- **Save as Word:** right-to-left, justified, in Faruma or the font you pick.
- **Scans and photos (experimental):** use *Read text*. Newspaper and magazine pages are split into columns and read right to left.

## Download
Go to the **Releases** page and download:
- **Windows:** `Thaana-OCR-…-Windows.exe` (no install; double-click it)
- **Mac:** the `.dmg` for your Mac

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
- Text inside long wrapped table cells may be split across extra rows.
- Always read through the result before using it for anything important.

## Build it yourself
Install Node.js (LTS), then in this folder run `npm install`, then:
- Windows build: `npm run dist:win`
- Mac build: double-click `build-mac.command`, or `npm run dist:mac-separate`

The scan-reading model is not part of this source repository. Without it, PDF mode works fully and scan reading uses the basic reader.

## Privacy
No file, text or image leaves your computer. This repository must never contain PDFs, student scripts, exam papers or fonts (see `.gitignore`).

## Licence
Apache License 2.0. See `LICENSE` and `NOTICE`.
