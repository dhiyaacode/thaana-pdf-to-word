//! Test hooks (only with `--features smoke`): runs the script named by THAANA_SMOKE_SCRIPT in the
//! page once it has loaded, and lets that script log, read test files and quit with a status code.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tauri::{webview::PageLoadEvent, AppHandle, Builder, Wry};

pub fn install(builder: Builder<Wry>) -> Builder<Wry> {
    builder.on_page_load(|webview, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        if let Ok(path) = std::env::var("THAANA_SMOKE_SCRIPT") {
            match std::fs::read_to_string(&path) {
                Ok(js) => {
                    let _ = webview.eval(js);
                }
                Err(e) => eprintln!("smoke: cannot read {path}: {e}"),
            }
        }
    })
}

#[tauri::command]
pub fn smoke_log(line: String) {
    println!("{line}");
}

#[tauri::command]
pub fn smoke_read(path: String) -> Result<String, String> {
    std::fs::read(&path).map(|b| B64.encode(b)).map_err(|e| format!("{path}: {e}"))
}

#[tauri::command]
pub fn smoke_exit(app: AppHandle, code: i32) {
    app.exit(code);
}
