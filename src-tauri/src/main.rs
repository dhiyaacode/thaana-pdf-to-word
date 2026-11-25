// Thaana PDF to Word: the Rust side of the app (files, dialogs, clipboard, Word export, labels).
// The interface (renderer/) runs in the system web view and calls these commands through
// renderer/bridge.js, which gives it the same `window.ocr` API the Electron version had.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod docx;
#[cfg(feature = "smoke")]
mod smoke;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
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

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

fn main() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init());
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        open_files, save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version
    ]);
    #[cfg(feature = "smoke")]
    let builder = smoke::install(builder.invoke_handler(tauri::generate_handler![
        open_files, save_docx, save_txt, copy_text, label_save, label_undo, label_show, label_count, app_version,
        smoke::smoke_log, smoke::smoke_read, smoke::smoke_exit
    ]));
    builder.run(tauri::generate_context!()).expect("error while running Thaana PDF to Word");
}
