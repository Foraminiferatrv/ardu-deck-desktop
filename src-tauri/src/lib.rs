mod deck;
mod discord;
mod twitch;

extern crate core;

use crate::deck::{init_deck, DeckEvent};
use crate::discord::init_discord;
use crate::twitch::auth_twitch;

use dotenv::{self, Dotenv};
use serde::Serialize;
use serde_json::Value;
use std::env;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub struct AppState {
    is_discord_auth: bool,
    is_twitch_auth: bool,
    is_micro_on: bool,
    is_live: bool,
    is_backlight_on: bool,
    viewers: i32,
    followers: i64,
}

#[allow(dead_code)]
impl AppState {
    fn new() -> AppState {
        AppState {
            is_discord_auth: false,
            is_twitch_auth: false,
            is_live: false,
            is_micro_on: false,
            is_backlight_on: true,
            followers: 0,
            viewers: 0,
        }
    }

    fn serialize(&self, event: DeckEvent) -> Vec<u8> {
        let mut message: Vec<&[u8]> = vec![b"*"];
        let viewers = format!("V{:0>3}", self.viewers.to_string());
        let followers = format!("F{:0>3}", self.followers.to_string());

        match event {
            DeckEvent::MicroUpdated(_) => {
                if self.is_micro_on {
                    message.push(b"M1");
                } else {
                    message.push(b"M0");
                }
            }

            DeckEvent::LiveUpdated => {
                if self.is_live {
                    message.push(b"L1");
                } else {
                    message.push(b"L0");
                }
            }

            DeckEvent::FollowersUpdate => {
                message.push(followers.as_bytes());
            }

            DeckEvent::ViewersUpdated => {
                message.push(viewers.as_bytes());
            }

            DeckEvent::StateUpdated => {
                //Micro serialize
                if self.is_micro_on {
                    message.push(b"M1");
                } else {
                    message.push(b"M0");
                }

                //Live serialize
                if self.is_live {
                    message.push(b"L1");
                } else {
                    message.push(b"L0");
                }

                //Viewers serialize
                message.push(viewers.as_bytes());

                //Followers serialize
                message.push(followers.as_bytes());
            }

            DeckEvent::BacklightUpdate(_) => {
                if self.is_backlight_on {
                    message.push(b"B1");
                } else {
                    message.push(b"B0");
                }
            }

            _ => {}
        }

        message.push(b"$");

        message.concat()
    }
}

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
            app.manage(Mutex::new(AppState::default()));

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
