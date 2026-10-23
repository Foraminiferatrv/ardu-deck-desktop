mod app;
mod deck;
mod discord;
mod twitch;

extern crate core;

use crate::app::AppState;
use crate::deck::init_deck;
use crate::discord::init_discord;
use crate::twitch::auth_twitch;

use dotenv::{self, Dotenv};
use serde_json::Value;
use std::env;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

#[tauri::command]
async fn get_state(app: AppHandle) -> Value {
    let state_mutex = app.state::<Mutex<AppState>>();
    let state = state_mutex.lock().unwrap();

    serde_json::json!(*state)
}

#[tauri::command]
async fn clear_store(app: AppHandle) {
    let store = app.store("store.json").unwrap();
    store.clear();

    app.request_restart();
}

#[tauri::command]
async fn toggle_backlight(app: AppHandle) {
    let state_mutex = app.state::<Mutex<AppState>>();
    let mut state = state_mutex.lock().unwrap();

    state.is_backlight_on = !state.is_backlight_on;

    app.emit("backlight-changed", state.is_backlight_on).ok();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let env: &str = include_str!("../../.env");
    let result: Dotenv = dotenv::from_read(env.as_bytes()).expect("Error loading env file");
    result.load();

    tauri::Builder::default()
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                window.hide().unwrap();
                api.prevent_close();
            }
            _ => {}
        }) //Prevent close window
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            app.manage(Mutex::new(AppState::new()));

            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
            println!("a new app instance was opened with {argv:?} and the deep link event was already triggered");
        }))
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            init_deck,
            init_discord,
            auth_twitch,
            get_state,
            clear_store,
            toggle_backlight
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
