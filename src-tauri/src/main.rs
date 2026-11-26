// Thaana PDF to Word: the Rust side of the app. It opens files and keeps their pages, reads the
// text of typed PDFs, draws pages, prepares scans for the reader and finishes their text, and
// does the dialogs, clipboard, Word export and handwriting labels (with crates/thaana-core).
// The interface (renderer/) runs in the system web view and calls these commands through
// renderer/bridge.js, which exposes them to it as `window.ocr`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod docx;
#[cfg(feature = "smoke")]
mod smoke;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::{fs, io::Write, path::PathBuf};
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

/// The pages of every open file, by number. The interface only ever holds page numbers.
#[derive(Default)]
struct Pages {
    next: u32,
    sources: HashMap<u32, thaana_core::Source>,
}
type PagesState = Arc<Mutex<Pages>>;

fn reader() -> &'static thaana_core::Reader {
    static READER: OnceLock<thaana_core::Reader> = OnceLock::new(); // loads the word list once
    READER.get_or_init(thaana_core::Reader::new)
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "camelCase", default)]
struct OpenOptions {
    drop_arabic: bool,
    join: bool,
    use_text: bool,
}

#[derive(Serialize)]
struct PageInfo {
    id: u32,
    kind: thaana_core::Kind,
    text: String,
    /// data: URL of a small JPEG ("" when the page cannot be drawn)
    thumb: String,
    /// false when the page cannot be drawn, so it cannot be read as a picture
    readable: bool,
}

#[derive(Serialize)]
struct OpenedFile {
    name: String,
    pages: Vec<PageInfo>,
    error: Option<String>,
}

fn open_bytes(state: &PagesState, name: String, bytes: Vec<u8>, opts: OpenOptions) -> OpenedFile {
    let ext = std::path::Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
    let read = thaana_core::ReadOptions { drop_arabic: opts.drop_arabic, join: opts.join };
    match reader().open(bytes, &ext, read, opts.use_text) {
        Err(e) => OpenedFile { name, pages: vec![], error: Some(e) },
        Ok(opened) => {
            let mut st = state.lock().unwrap();
            let pages = opened
                .into_iter()
                .map(|o| {
                    st.next += 1;
                    let id = st.next;
                    let readable = o.source.is_some();
                    if let Some(src) = o.source {
                        st.sources.insert(id, src);
                    }
                    let thumb = o.thumb.map(|j| format!("data:image/jpeg;base64,{}", B64.encode(j))).unwrap_or_default();
                    PageInfo { id, kind: o.kind, text: o.text, thumb, readable }
                })
                .collect();
            OpenedFile { name, pages, error: None }
        }
    }
}

/// Asks for files and opens them.
#[tauri::command]
async fn open_files(app: AppHandle, state: tauri::State<'_, PagesState>, opts: OpenOptions) -> Res<Vec<OpenedFile>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose scans, photos or PDFs")
        .add_filter("Images and PDFs", &["pdf", "png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp", "gif"])
        .blocking_pick_files()
        .unwrap_or_default();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        picked
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|path| {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match fs::read(&path) {
                    Ok(bytes) => open_bytes(&state, name, bytes, opts),
                    Err(e) => OpenedFile { name, pages: vec![], error: Some(e.to_string()) },
                }
            })
            .collect()
    })
    .await
    .map_err(err)
}

/// Opens a file dropped or pasted on the window: its bytes are the raw request body; its name
/// and the options are JSON in the `x-options` header.
#[tauri::command]
async fn open_document(state: tauri::State<'_, PagesState>, request: tauri::ipc::Request<'_>) -> Res<OpenedFile> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("expected the file as raw bytes".into()) };
    #[derive(Deserialize)]
    struct Head {
        name: String,
        #[serde(flatten)]
        opts: OpenOptions,
    }
    let head: Head = request.headers().get("x-options").and_then(|v| v.to_str().ok()).map(percent_decode).and_then(|v| serde_json::from_str(&v).ok()).ok_or("missing file name")?;
    let (bytes, state) = (bytes.clone(), state.inner().clone());
    tauri::async_runtime::spawn_blocking(move || open_bytes(&state, head.name, bytes, head.opts)).await.map_err(err)
}

/// Undoes encodeURIComponent (the header carries the file name, which may contain Thaana).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Forgets pages the interface removed, freeing their memory.
#[tauri::command]
fn pages_forget(state: tauri::State<'_, PagesState>, ids: Vec<u32>) {
    let mut st = state.lock().unwrap();
    for id in ids {
        st.sources.remove(&id);
    }
}

fn page_source(state: &PagesState, id: u32) -> Res<thaana_core::Source> {
    state.lock().unwrap().sources.get(&id).cloned().ok_or_else(|| "this page cannot be drawn, so it cannot be read".into())
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

#[derive(Serialize)]
struct ScanResult {
    lines: Vec<String>, // base64 PNG per text line
    boxes: Vec<thaana_core::scan::LineBox>,
    pitch: f64,
    skew: f64,
    blk: Option<Vec<usize>>,
}

/// Draws a page, cleans it up and cuts it into text lines for the reader (crates/thaana-core).
#[tauri::command]
async fn scan_prepare(state: tauri::State<'_, PagesState>, id: u32, rotation: i32, enhance: bool, columns: bool) -> Res<ScanResult> {
    let src = page_source(state.inner(), id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let page = src.for_reading().ok_or("this page cannot be drawn")?;
        let p = thaana_core::scan::prepare(page, rotation, enhance, columns);
        Ok(ScanResult { lines: p.lines.iter().map(|l| B64.encode(l)).collect(), boxes: p.boxes, pitch: p.pitch, skew: p.skew, blk: p.blk })
    })
    .await
    .map_err(err)?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finish {
    /// what the reader recognised on each line
    lines: Vec<String>,
    boxes: Vec<thaana_core::scan::LineBox>,
    pitch: f64,
    blk: Option<Vec<usize>>,
    join: bool,
    with_english: bool,
}

/// The finished text of a scanned page: the reader's noise removed, lines joined into
/// paragraphs, words repaired with the word list.
#[tauri::command]
async fn ocr_finish(f: Finish) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || thaana_core::ocr::finish(&f.lines, &f.boxes, f.pitch, f.blk.as_deref(), f.join, f.with_english, reader().repairer()))
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
async fn scan_prepare_label(state: tauri::State<'_, PagesState>, id: u32, rotation: i32) -> Res<(Vec<LabelLine>, f64)> {
    let src = page_source(state.inner(), id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let page = src.for_reading().ok_or("this page cannot be drawn")?;
        let (lines, pitch) = thaana_core::scan::prepare_label(page, rotation);
        Ok((lines.into_iter().map(|(a, b)| LabelLine { orig: B64.encode(a), show: B64.encode(b) }).collect(), pitch))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(PagesState::default());
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        open_files, open_document, pages_forget, scan_prepare, ocr_finish, scan_prepare_label,
        save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version
    ]);
    #[cfg(feature = "smoke")]
    let builder = smoke::install(builder.invoke_handler(tauri::generate_handler![
        open_files, open_document, pages_forget, scan_prepare, ocr_finish, scan_prepare_label,
        save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version,
        smoke::smoke_log, smoke::smoke_read, smoke::smoke_exit
    ]));
    builder.run(tauri::generate_context!()).expect("error while running Thaana PDF to Word");
}
