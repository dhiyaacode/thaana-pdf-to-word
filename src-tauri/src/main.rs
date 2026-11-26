// Thaana PDF to Word: the Rust side of the app (files, dialogs, clipboard, Word export, labels).
// The interface (renderer/) runs in the system web view and calls these commands through
// renderer/bridge.js, which exposes them to it as `window.ocr`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod docx;
#[cfg(feature = "smoke")]
mod smoke;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::OnceLock};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, String>;
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Asks where to save. In smoke-test builds THAANA_SAVE_TO answers instead of the dialog.
fn ask_save_path(app: &AppHandle, title: &str, name: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
    #[cfg(feature = "smoke")]
    if let Ok(p) = std::env::var("THAANA_SAVE_TO") {
        return Some(PathBuf::from(p));
    }
    app.dialog().file().set_title(title).set_file_name(name).add_filter(filter.0, filter.1).blocking_save_file()?.into_path().ok()
}

#[derive(Serialize)]
struct OpenedFile {
    name: String,
    data: String, // base64, as the interface expects
    ext: String,
}

#[tauri::command]
async fn open_files(app: AppHandle) -> Res<Vec<OpenedFile>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose scans, photos or PDFs")
        .add_filter("Images and PDFs", &["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp", "pdf"])
        .blocking_pick_files()
        .unwrap_or_default();
    let mut out = Vec::new();
    for f in picked {
        let path = f.into_path().map_err(err)?;
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(OpenedFile { name, data: B64.encode(fs::read(&path).map_err(err)?), ext });
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDocx {
    text: String,
    font_name: String,
    font_size: Option<f64>,
    suggested_name: Option<String>,
}

#[tauri::command]
async fn save_docx(app: AppHandle, opts: SaveDocx) -> Res<Option<String>> {
    let name = format!("{}.docx", opts.suggested_name.as_deref().unwrap_or("thaana-text"));
    let Some(path) = ask_save_path(&app, "Save as Word document", &name, ("Word document", &["docx"])) else { return Ok(None) };
    let bytes = docx::build(&opts.text, &opts.font_name, opts.font_size.unwrap_or(14.0))?;
    fs::write(&path, bytes).map_err(err)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveTxt {
    text: String,
    suggested_name: Option<String>,
}

#[tauri::command]
async fn save_txt(app: AppHandle, opts: SaveTxt) -> Res<Option<String>> {
    let name = format!("{}.txt", opts.suggested_name.as_deref().unwrap_or("thaana-text"));
    let Some(path) = ask_save_path(&app, "Save as text file", &name, ("Text", &["txt"])) else { return Ok(None) };
    fs::write(&path, format!("\u{FEFF}{}", opts.text)).map_err(err)?; // BOM so Notepad/Word pick UTF-8
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Copies plain text plus right-to-left HTML, so Word pastes Faruma, justified, RTL.
#[tauri::command]
fn copy_text(text: String) -> Res<bool> {
    let paras: String = text
        .split('\n')
        .map(|l| {
            let l = escape_html(l);
            format!(r#"<p dir="rtl" style="text-align:justify;font-family:Faruma,'MV Boli';font-size:14pt">{}</p>"#, if l.is_empty() { "&nbsp;".into() } else { l })
        })
        .collect();
    let mut cb = arboard::Clipboard::new().map_err(err)?;
    cb.set_html(format!(r#"<html><body dir="rtl">{paras}</body></html>"#), Some(text)).map_err(err)?;
    Ok(true)
}

// ---- handwriting labelling: one PNG + one line of text per labelled line ----
fn label_dir(app: &AppHandle) -> Res<PathBuf> {
    let d = app.path().document_dir().map_err(err)?.join("Thaana-handwriting-labels");
    fs::create_dir_all(d.join("images")).map_err(err)?;
    Ok(d)
}
fn valid_id(id: &str) -> bool {
    (4..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) // ids become file names
}

#[derive(Deserialize)]
struct Label {
    id: String,
    png: String,
    text: String,
    meta: Option<String>,
}

#[tauri::command]
fn label_save(app: AppHandle, o: Label) -> Res<String> {
    if !valid_id(&o.id) {
        return Err("bad label id".into());
    }
    let d = label_dir(&app)?;
    fs::write(d.join("images").join(format!("{}.png", o.id)), B64.decode(&o.png).map_err(err)?).map_err(err)?;
    let text: String = o.text.split(['\t', '\r', '\n']).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let mut f = fs::OpenOptions::new().create(true).append(true).open(d.join("labels.tsv")).map_err(err)?;
    writeln!(f, "{}\t{}\t{}", o.id, text, o.meta.unwrap_or_default()).map_err(err)?;
    Ok(d.to_string_lossy().into_owned())
}

#[tauri::command]
fn label_undo(app: AppHandle, id: String) -> Res<()> {
    if !valid_id(&id) {
        return Ok(());
    }
    let d = label_dir(&app)?;
    let _ = fs::remove_file(d.join("images").join(format!("{id}.png")));
    let f = d.join("labels.tsv");
    if let Ok(s) = fs::read_to_string(&f) {
        let keep: String = s.lines().filter(|l| !l.is_empty() && !l.starts_with(&format!("{id}\t"))).map(|l| format!("{l}\n")).collect();
        fs::write(&f, keep).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn label_show(app: AppHandle) -> Res<String> {
    let d = label_dir(&app)?;
    app.opener().open_path(d.to_string_lossy(), None::<&str>).map_err(err)?;
    Ok(d.to_string_lossy().into_owned())
}

#[tauri::command]
fn label_count(app: AppHandle) -> usize {
    label_dir(&app).ok().and_then(|d| fs::read_to_string(d.join("labels.tsv")).ok()).map(|s| s.lines().filter(|l| !l.is_empty()).count()).unwrap_or(0)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ReadPdfOptions {
    drop_arabic: bool,
    join: bool,
}

/// Reads the text of every page of a PDF (crates/thaana-core). The PDF comes as the raw request
/// body; the options as JSON in the `x-options` header.
#[tauri::command]
async fn pdf_read(request: tauri::ipc::Request<'_>) -> Res<Vec<thaana_core::Page>> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("expected the PDF as raw bytes".into()) };
    let opts: ReadPdfOptions = request.headers().get("x-options").and_then(|v| v.to_str().ok()).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default();
    let bytes = bytes.clone();
    static READER: OnceLock<thaana_core::Reader> = OnceLock::new(); // loads the word list once
    tauri::async_runtime::spawn_blocking(move || {
        READER.get_or_init(thaana_core::Reader::new).read(&bytes, thaana_core::ReadOptions { drop_arabic: opts.drop_arabic, join: opts.join })
    })
    .await
    .map_err(err)?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanOptions {
    w: usize,
    h: usize,
    #[serde(default)]
    rotation: i32,
    #[serde(default)]
    enhance: bool,
    #[serde(default)]
    columns: bool,
}

/// The page's RGBA pixels (raw request body) and the options (`x-options` header).
fn scan_input(request: &tauri::ipc::Request<'_>) -> Res<(Vec<u8>, ScanOptions)> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("expected the page pixels as raw bytes".into()) };
    let opts: ScanOptions = request.headers().get("x-options").and_then(|v| v.to_str().ok()).and_then(|v| serde_json::from_str(v).ok()).ok_or("missing scan options")?;
    if opts.w == 0 || opts.h == 0 || bytes.len() < opts.w * opts.h * 4 {
        return Err("page size does not match the pixels".into());
    }
    Ok((bytes.clone(), opts))
}

#[derive(Serialize)]
struct ScanResult {
    lines: Vec<String>, // base64 PNG per text line
    boxes: Vec<thaana_core::scan::LineBox>,
    pitch: f64,
    skew: f64,
    blk: Option<Vec<usize>>,
}

/// Cleans up a scanned page and cuts it into text lines (crates/thaana-core/src/scan.rs).
#[tauri::command]
async fn scan_prepare(request: tauri::ipc::Request<'_>) -> Res<ScanResult> {
    let (rgba, o) = scan_input(&request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let p = thaana_core::scan::prepare(&rgba, o.w, o.h, o.rotation, o.enhance, o.columns);
        ScanResult { lines: p.lines.iter().map(|l| B64.encode(l)).collect(), boxes: p.boxes, pitch: p.pitch, skew: p.skew, blk: p.blk }
    })
    .await
    .map_err(err)
}

#[derive(Serialize)]
struct LabelLine {
    orig: String, // base64 PNG of the line as scanned
    show: String, // base64 PNG with notebook ruling removed
}

/// Text lines of a handwritten page for the labeller.
#[tauri::command]
async fn scan_prepare_label(request: tauri::ipc::Request<'_>) -> Res<(Vec<LabelLine>, f64)> {
    let (rgba, o) = scan_input(&request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (lines, pitch) = thaana_core::scan::prepare_label(&rgba, o.w, o.h, o.rotation);
        (lines.into_iter().map(|(a, b)| LabelLine { orig: B64.encode(a), show: B64.encode(b) }).collect(), pitch)
    })
    .await
    .map_err(err)
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

fn main() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init());
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        open_files, save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version, pdf_read, scan_prepare, scan_prepare_label
    ]);
    #[cfg(feature = "smoke")]
    let builder = smoke::install(builder.invoke_handler(tauri::generate_handler![
        open_files, save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version, pdf_read, scan_prepare, scan_prepare_label,
        smoke::smoke_log, smoke::smoke_read, smoke::smoke_exit
    ]));
    builder.run(tauri::generate_context!()).expect("error while running Thaana PDF to Word");
}
