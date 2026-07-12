// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tauri::State;

struct EngineState(Mutex<matbox_core::Engine>);

#[tauri::command]
fn eval(code: &str, engine: State<EngineState>) -> Result<String, String> {
    let mut eng = engine.0.lock().map_err(|e| e.to_string())?;
    match eng.eval(code) {
        Ok(Some(v)) => Ok(matbox_core::builtins::format_value(&v)),
        Ok(None) => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn clear(engine: State<EngineState>) -> Result<(), String> {
    let mut eng = engine.0.lock().map_err(|e| e.to_string())?;
    eng.workspace.clear();
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(EngineState(Mutex::new(matbox_core::Engine::new())))
        .invoke_handler(tauri::generate_handler![eval, clear])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
