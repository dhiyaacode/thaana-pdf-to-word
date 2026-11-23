# Thaana PDF to Word

Free desktop app that turns Dhivehi (Thaana) PDFs, scans and photos into editable text, ready to paste into Word or save as a .docx.

**PDF with typed text (fast):** drop the PDF in. The app takes the text straight out of it with the letters in the right order, fixes misplaced fili where it is sure, and you can copy or save it as Word. No reading step.
**Scans and photos:** use **Read text** (see below). Handwriting is not supported yet.

## Using it
1. **Open scans / PDFs…** (or drag files into the window, or paste a screenshot with Ctrl+V / ⌘V).
2. Click **Read text**.
3. Check the text, especially the fili, and fix anything that's wrong.
4. **Copy all** and paste into Word, or **Save as Word**. The file is right-to-left, justified, in Faruma (or the font you pick).

Tips
- Scans at 200–300 dpi work best. Phone photos are fine if the page is flat, well lit and fills the frame.
- Use ⟳ on a page to rotate it if the photo is sideways.
- Tick **Page also has English text** only when a page mixes English and Dhivehi.
- Untick **Join lines into paragraphs** to keep the page's original line breaks (good for lists and numbered questions).
- Everything runs on your computer. Nothing is uploaded.

## First launch (the app is not signed)
**Windows:** run `the Windows .exe`. If SmartScreen says "Windows protected your PC", click **More info → Run anyway**. No installation needed; you can keep the .exe on the Desktop.

**Mac:** open the .dmg and drag Thaana PDF to Word to Applications. The first time, macOS will block it. Go to **System Settings → Privacy & Security**, scroll down, and click **Open Anyway** next to Thaana PDF to Word.

## Building the Mac version (on a Mac)
1. Install Node.js (LTS) from https://nodejs.org.
2. Double-click `build-mac.command` in this folder. If macOS won't run it, right-click → Open.
3. The finished `.dmg` appears in the `dist` folder. It runs on both Intel and Apple-chip Macs.

## Accuracy (v0.1, on test pages)
About 96% of characters and 73–77% of whole words come out correct on clean prints, scans, photocopies and phone photos. Most mistakes are a single wrong or missing fili, so read through before using the text.

## Newspapers and magazines
Scans and screenshots with several columns are split into blocks and read right to left, top to bottom. Pictures are skipped. Turn this off with “Newspaper / magazine columns” if a plain page reads wrongly.
