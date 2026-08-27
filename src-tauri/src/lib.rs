extern crate core;

const BAUDRATE: u32 = 115200;

use core::time;
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use dotenv::{self, Dotenv};
use enigo::Direction::{Click, Press, Release};
use enigo::{Button, Enigo, Key, Keyboard};
use futures::stream::SplitStream;
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serial2::SerialPort;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, thread};
use tauri::{async_runtime::Mutex as AsyncMutex, AppHandle, Manager};
use tauri::{Emitter, Listener};
use tauri_plugin_store::StoreExt;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use twitch_api::eventsub::channel::chat::message::MessageType::Text;
use twitch_api::eventsub::channel::ChannelFollowV2;
use twitch_api::eventsub::stream::{StreamOfflineV1, StreamOnlineV1};
use twitch_api::eventsub::{Event, EventsubWebsocketData, SessionData, Transport, WelcomePayload};
use twitch_api::helix::eventsub::{CreateEventSubSubscription, CreateEventSubSubscriptionRequest};

use twitch_api::twitch_oauth2::{DeviceUserTokenBuilder, TwitchToken, UserToken};
use twitch_api::{helix, twitch_oauth2, HelixClient};
use twitch_oauth_token::{RefreshToken, Scope};

use std::sync::mpsc;
use zerocopy::IntoBytes;

/// action to perform on received message
enum WsAction {
    /// do nothing with the message
    Nothing,
    /// reset the timeout and keep the connection alive
    ResetKeepalive,
    /// kill predecessor and swap the handle
    KillPredecessor,
    /// spawn successor and await death signal
    AssignSuccessor,
}

#[derive(Debug)]
enum DeckButton {
    B3,
    B4,
    B5,
    B6,
    B7,
    B8,
    B9,
}

#[derive(PartialEq, Debug)]
enum DeckEvent {
    StateUpdated,
    MicroUpdated(bool),
    MicroOn,
    MicroOff,
    LiveUpdated,
    ViewersUpdated,
    FollowersUpdate,
    TauriEvent,
    DiscordAuthenticated,
    None,
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
struct AppState {
    is_discord_auth: bool,
    is_twitch_auth: bool,
    is_micro_on: bool,
    is_live: bool,
    viewers: i32,
    followers: i64,
}

impl AppState {
    fn new() -> AppState {
        AppState {
            is_discord_auth: false,
            is_twitch_auth: false,
            is_live: false,
            is_micro_on: false,
            followers: 0,
            viewers: 0,
        }
    }

    fn set_is_micro_on(&mut self, new_value: bool) {
        self.is_micro_on = new_value;
    }
    fn set_is_live(&mut self, new_value: bool) {
        self.is_live = new_value;
    }
    fn set_viewers(&mut self, new_value: i32) {
        self.viewers = new_value;
    }
    fn set_followers(&mut self, new_value: i64) {
        self.followers = new_value;
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

            _ => {}
        }

        message.push(b"$");

        message.concat()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthorizeData {
    code: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthorizeResponse {
    cmd: String,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // evt: Option<String>,
    data: DiscordAuthorizeData,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthenticateData {
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<serde_json::Number>,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthenticateResponse {
    cmd: String,
    data: DiscordAuthenticateData,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordTokenResponse {
    access_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RPCMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    evt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DiscordVoiceData {
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    deaf: bool,
}

#[tauri::command]
async fn init_discord(app: AppHandle) -> Result<(), ()> {
    println!("init discord");
    let state_mutex = app.state::<Mutex<AppState>>();

    let client_id = env::var("CLIENT_ID").expect("No CLIENT_ID in .env file");
    let client_secret = env::var("DISCROD_SECRET").expect("No DISCROD_SECRET in .env file");

    let store = app.store("store.json").unwrap();
    let mut access_token = store.get("access_token");

    let mut client = DiscordIpcClient::new(client_id.clone());

    client.connect().ok();

    if access_token.is_none() {
        //AUTHORIZE STRART
        let auth_data = serde_json::json!({
          "nonce": "null",
          "args": {
            "client_id": client_id,
            "scopes": ["rpc"]
          },
          "cmd": "AUTHORIZE"
        });

        match client.send(auth_data, 1) {
            Err(e) => {
                println!("authorize Error: {:?}\n", e)
            }
            _ => {}
        };

        match client.recv() {
            Err(e) => println!("Recv Error: {:?}\n", e),
            Ok((_code, data)) => {
                let de_res: DiscordAuthorizeResponse = serde_json::from_value(data).unwrap();

                let client = reqwest::Client::new();

                let form = reqwest::multipart::Form::new()
                    .text("client_id", client_id)
                    .text("grant_type", "authorization_code")
                    .text("client_secret", client_secret)
                    .text("code", de_res.data.code);

                let req = client
                    .post("https://discord.com/api/v10/oauth2/token")
                    .multipart(form)
                    .header("Content-Type", "application/x-www-form-urlencoded");

                let res = req.send().await.unwrap();

                let token_res = res.json::<DiscordTokenResponse>().await.expect("Error deserializing access_token");
                store.set("access_token", json!(token_res.access_token));
                access_token = store.get("access_token");
            }
        }
    };

    //Try to Auth with access token
    let authenticate_date = serde_json::json!({
      "nonce": "null",
      "args": {
          "access_token": access_token
      },
      "cmd": "AUTHENTICATE"
    });

    client.send(authenticate_date, 1).expect("Error while trying to auth!");
    match client.recv() {
        Err(e) => {
            println!("Auth recv Error: {:?}\n", e);
            store.delete("access_token");
            access_token = store.get("access_token");
        }
        Ok((_res_code, data)) => {
            let de_res: DiscordAuthenticateResponse = serde_json::from_value(data).unwrap();
            let code = de_res.data.code.unwrap_or(serde_json::Number::from(0));
            let mut state = state_mutex.lock().unwrap();

            if code != serde_json::Number::from(4009) {
                state.is_discord_auth = true;
                app.emit("discord-auth", true).ok();
            } else {
                //Rearuth if the code is invalid
                store.delete("access_token");
                access_token = store.get("access_token");
                app.emit("discord-auth", false).ok();
                state.is_discord_auth = false;
            }
        }
    };

    if access_token.is_none() {
        return Err(());
    }

    //Get voice settings
    let settings_req_data = serde_json::json!({
      "nonce": "null",
      "args": {
          "access_token": access_token
      },
      "cmd": "SUBSCRIBE",
      "evt":"VOICE_SETTINGS_UPDATE"
    });

    match client.send(settings_req_data, 1) {
        Err(e) => println!("send Error: {:?}", e),
        Ok(data) => println!("send: {:?}", data),
    };

    loop {
        match client.recv() {
            Err(e) => println!("Recv settings Error: {:?}\n", e),
            Ok((_code, data)) => {
                let msg: RPCMessage = serde_json::from_value(data).unwrap_or_default();
                let voice_status: DiscordVoiceData = serde_json::from_value(msg.data.unwrap_or_default()).unwrap_or_default();

                let is_mute = voice_status.mute;

                let mut state = state_mutex.lock().unwrap();

                state.is_micro_on = !is_mute;

                app.emit("mic-change", !is_mute).ok();
            }
        };
    }
}

fn read_deck(app: AppHandle, port: Arc<Mutex<SerialPort>>, tx: mpsc::Sender<DeckEvent>) {
    let mut read_buffer = [0; 8];
    let mut en = Enigo::new(&enigo::Settings::default()).unwrap();
    let state_mutex = app.state::<Mutex<AppState>>();

    loop {
        // println!("Read thread");
        thread::sleep(time::Duration::from_millis(60));

        {
            let port = port.lock().unwrap();
            let read = port.read(&mut read_buffer).unwrap_or_else(|_e| 0);

            if read != 0 {
                let [first, second, _third, fourth, ..] = read_buffer;

                if first == b'B' {
                    if second == b'3' {
                        // en.key(Key::F13, Press).unwrap();
                        // thread::sleep(time::Duration::from_millis(60));
                        // en.key(Key::F13, Release).unwrap();
                        let state = state_mutex.lock().unwrap();

                        println!("state: {:?}", state);

                        if state.is_discord_auth {
                            if fourth == b'1' {
                                if !state.is_micro_on {
                                    en.key(Key::F13, Click).unwrap();
                                }
                            } else if fourth == b'0' {
                                if state.is_micro_on {
                                    en.key(Key::F13, Click).unwrap();
                                }
                            }
                        } else {
                            en.key(Key::F13, Click).unwrap();
                        }
                        // tx.send(DeckEvent::MicroUpdated).unwrap();
                    }
                    if second == b'4' {
                        println!("MACRO 4");
                        en.key(Key::F14, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52)); //OBS has a key listener with 50ms interval
                        en.key(Key::F14, Release).unwrap();
                    }
                    if second == b'5' {
                        println!("MACRO 5");
                        en.key(Key::F15, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F15, Release).unwrap();
                    }
                    if second == b'6' {
                        println!("MACRO 6");
                        en.key(Key::F16, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F16, Release).unwrap();
                    }
                    if second == b'7' {
                        println!("MACRO 7");
                        en.key(Key::F17, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F17, Release).unwrap();
                    }
                    if second == b'8' {
                        println!("MACRO 8");
                        en.key(Key::F18, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F18, Release).unwrap();
                    }
                    if second == b'9' {
                        println!("MACRO 9");
                        en.key(Key::F19, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F19, Release).unwrap();
                    }
                }
            };
            // thread::sleep(time::Duration::from_millis(60));
        }
    }
}

fn write_deck(app: AppHandle, port: Arc<Mutex<SerialPort>>, event: DeckEvent) {
    // let message = b"*M1L1V204F001$";
    println!("Event2: {:?}", event);
    let state = app.state::<Mutex<AppState>>();
    let state = state.lock().unwrap();

    let message = state.serialize(event);
    let port = port.lock().unwrap();

    println!("Message: {:?}", message);

    for byte in message {
        println!("Write: {}", byte);
        port.write(byte.as_bytes()).expect("Error writing bytes");
        port.flush().unwrap();

        thread::sleep(time::Duration::from_millis(35)); // Delay in write: 20 x 14 = 280ms
    }
}

#[tauri::command]
async fn init_deck(app: AppHandle) -> Result<(), ()> {
    println!("Init deck");

    let mut raw_port = SerialPort::open("COM3", BAUDRATE).unwrap();
    raw_port.set_read_timeout(Duration::new(0, 0)).unwrap();

    let port: Arc<Mutex<SerialPort>> = Arc::new(Mutex::new(raw_port));

    let (tx, rx) = mpsc::channel::<DeckEvent>();

    // let state: State<'_, Mutex<AppState>> = app.state::<Mutex<AppState>>().clone();
    // let state = Arc::new(state_mutex);

    let read_app = app.clone();

    thread::spawn({
        let reader_port = Arc::clone(&port);
        let reader_tx = tx.clone();

        move || read_deck(read_app, reader_port, reader_tx)
    });

    let tx_mic = tx.clone();
    app.listen_any("mic-change", move |e| {
        let data = e.payload();

        let _res = &tx_mic
            .send(if data == "true" {
                DeckEvent::MicroUpdated(true)
            } else {
                DeckEvent::MicroUpdated(false)
            })
            .unwrap();
    });

    let tx_followers = tx.clone();
    app.listen_any("followers-changed", move |_e| {
        let _res = &tx_followers.send(DeckEvent::FollowersUpdate).unwrap();
    });

    let tx_clone = tx.clone();
    app.listen_any("live-changed", move |_e| {
        let _res = &tx_clone.send(DeckEvent::LiveUpdated).unwrap();
    });

    let tx_clone = tx.clone();
    app.listen_any("viewers-changed", move |_e| {
        let _res = &tx_clone.send(DeckEvent::ViewersUpdated).unwrap();
    });

    loop {
        let write_app = app.clone();

        let event = match rx.try_recv() {
            Ok(val) => val,
            Err(_) => DeckEvent::None,
        };

        if event != DeckEvent::None {
            let state_mutex = write_app.state::<Mutex<AppState>>();

            match event {
                DeckEvent::MicroUpdated(is_micro_on) => {
                    let mut state = state_mutex.lock().unwrap();
                    state.is_micro_on = is_micro_on;
                }

                _ => {}
            }

            let writer_port = Arc::clone(&port);
            write_deck(write_app, writer_port, event);

            tx.send(DeckEvent::None).unwrap();
        }
    }
}

#[tauri::command]
async fn auth_twitch(app: AppHandle) {
    println!("Auth Twitch.");

    let state_mutex = app.state::<Mutex<AppState>>();
    let store = app.store("store.json").unwrap();

    let access_token_value = store.get("twitch_access_token");

    let reqwest = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

    if access_token_value.is_none() {
        let twitch_client_id = env::var("TWITCH_CLIENT_ID").expect("No TWITCH_CLIENT_ID in .env file");
        let client_id = twitch_oauth2::ClientId::new(twitch_client_id);

        let mut builder = DeviceUserTokenBuilder::new(
            client_id,
            vec![twitch_oauth2::Scope::ModeratorReadFollowers, twitch_oauth2::Scope::UserReadBroadcast],
        );
        let code = builder.start(&reqwest).await.unwrap();

        tauri_plugin_opener::open_url(&code.verification_uri, None::<&str>).unwrap();

        println!("Please go to {0}", code.verification_uri);
        println!("Waiting for user to authorize, time left: {0}", code.expires_in);

        let token = builder.wait_for_code(&reqwest, tokio::time::sleep).await.unwrap();
        store.set("twitch_access_token", json!(token.access_token));
        store.set("twitch_refresh_token", json!(&token.refresh_token));
    }

    let access_token_value = store.get("twitch_access_token").unwrap();
    let access_token = serde_json::from_value(access_token_value).unwrap();
    // let refresh_token_value = store.get("twitch_refresh_token").unwrap();
    // let refresh_token: Option<RefreshToken> = serde_json::from_value(refresh_token_value).unwrap();

    let twitch_token_data = UserToken::from_token(&reqwest, access_token).await;

    match twitch_token_data {
        Ok(twitch_token) => {
            let user_id = twitch_token.user_id.as_str();
            // let user_id = twitch_token.
            let access_token = &twitch_token.access_token;
            // twitch_token.is_elapsed()
            // twitch_token.refresh_token(&reqwest).await.unwrap();

            // store.set("twitch_access_token", json!(twitch_token.access_token));
            {
                let mut state = state_mutex.lock().unwrap();
                state.is_twitch_auth = true;
            }

            app.emit("twitch-auth", true).ok();

            let helix_client: HelixClient<reqwest::Client> = HelixClient::default();
            let channel = helix_client.get_channel_from_id(user_id, &twitch_token).await.unwrap().unwrap();
            // let broa

            let followers = helix_client.get_total_channel_followers(channel.broadcaster_id, &twitch_token).await.unwrap();

            {
                let mut state = state_mutex.lock().unwrap();

                state.followers = followers;
                app.emit("followers-changed", followers).ok();
            }

            let broadcaster_name = channel.broadcaster_name.as_str();

            let live_data: Vec<helix::search::Channel> = helix_client
                .search_channels(broadcaster_name, false, &twitch_token)
                .try_collect()
                .await
                .unwrap();

            {
                let is_live = live_data[0].is_live;
                // println!("is live: {:?}\n", is_live);
                let mut state = state_mutex.lock().unwrap();

                state.is_live = is_live;
                app.emit("live-changed", is_live).ok();
            }

            let mut is_subscribed = false;
            let config = Some(
                WebSocketConfig::default()
                    .max_message_size(Some(64 << 20)) // 64 MiB
                    .max_frame_size(Some(16 << 20)) // 16 MiB
                    .accept_unmasked_frames(false),
            );

            let stream_data: Vec<helix::streams::Stream> = helix_client
                .get_streams_from_ids(&[user_id][..].into(), &twitch_token)
                .try_collect()
                .await
                .unwrap();

            println!("Streams: {:?}", stream_data);

            {
                let viewers = stream_data[0].viewer_count as i32;
                // println!("is live: {:?}\n", is_live);
                let mut state = state_mutex.lock().unwrap();

                state.viewers = viewers;
                app.emit("viewers-changed", viewers).ok();
            }

            let url = twitch_api::TWITCH_EVENTSUB_WEBSOCKET_URL.clone();

            //Connnect to the twitch socket

            let mut socket = tokio_tungstenite::connect_async_with_config(url, config, false).await.unwrap().0.split().1;

            let mut session_id: Option<String> = None;

            loop {
                if !is_subscribed {
                    match &session_id {
                        Some(_session) => {
                            //Send
                            // let subscription = StreamOnlineV1();

                            // let transport: Transport = Transport::websocket(session);
                            // let _event_info: CreateEventSubSubscription<_> = helix_client
                            //     .create_eventsub_subscription(subscription, transport, &twitch_token)
                            //     .await
                            //     .expect("Subscribe to event failed");

                            is_subscribed = true;
                        }
                        _ => {}
                    }
                }

                //Rec
                let frame_result = match socket.next().await {
                    Some(Ok(message)) => {
                        match message {
                            Message::Close(frame) => {
                                let reason = frame.map(|frame| frame.reason).unwrap_or_default();

                                println!("Socket closed: {:?}", reason);

                                None
                                // Some(messsage);
                                // Err(eyre::eyre!("websocket stream closed unexpectedly with reason {reason}"))
                            }
                            Message::Frame(_) => unreachable!(),
                            Message::Ping(_) | Message::Pong(_) => None,

                            Message::Binary(_) => unimplemented!(),
                            Message::Text(payload) => Some(payload),
                        }
                    }

                    Some(Err(e)) => None,
                    None => None,
                };

                match frame_result {
                    Some(frame) => {
                        let event = Event::parse_websocket(&frame).expect("parsing error");
                        println!("payload: {:?} \n", event);

                        match event {
                            EventsubWebsocketData::Welcome {
                                payload: WelcomePayload { session },
                                ..
                            } => {
                                session_id = Some(session.id.to_string());
                                // println!("session: {:?} \n", &session_data);

                                // process_welcome(
                                //     &self.subscribed,
                                //     &self.token,
                                //     self.helix_client,
                                //     &self.user_id,
                                //     session,
                                // )
                                // .await?;
                                // Ok(WsAction::KillPredecessor)
                            }

                            _ => {
                                // Ok(WsAction::Nothing)
                            }
                        }
                    }
                    None => {
                        // println!("Stream error");
                        // Ok(WsAction::Nothing)
                    }
                };
            }
        }
        Err(e) => {
            app.emit("twitch-auth", false).ok();
            println!("Error token auth: {:?}", e);
        }
    };
}

enum TauriDeckEvent {
    // StateUpdated,
    MicroUpdated(bool),
    LiveUpdated(bool),
    ViewersUpdated(i32),
    FollowersUpdate(i32),
    MacroUpdated(DeckButton),
    None,
}

#[tauri::command]
async fn get_state(app: AppHandle) -> Value {
    let state_mutex = app.state::<Mutex<AppState>>();
    let state = state_mutex.lock().unwrap();

    serde_json::json!(*state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let env: &str = include_str!("../../.env");
    let result: Dotenv = dotenv::from_read(env.as_bytes()).expect("Error loading env file");
    result.load();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            app.manage(Mutex::new(AppState::default()));

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
            println!("a new app instance was opened with {argv:?} and the deep link event was already triggered");
        }))
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![init_deck, init_discord, auth_twitch, get_state])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
