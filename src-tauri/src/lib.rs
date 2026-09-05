extern crate core;

const BAUDRATE: u32 = 115200;

use core::time;

use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use dotenv::{self, Dotenv};
use enigo::Direction::{Click, Press, Release};
use enigo::{Button, Enigo, Key, Keyboard};
use eyre::Context;
use futures::TryStreamExt;
use futures::{stream::SplitStream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serial2::SerialPort;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use std::{env, thread};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{async_runtime::Mutex as AsyncMutex, AppHandle, Manager};
use tauri::{Emitter, Listener};
use tauri_plugin_store::StoreExt;
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedSender};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message as WsMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use twitch_api::eventsub::{
    self,
    channel::{ChannelBanV1, ChannelFollowV2, ChannelUnbanV1},
    stream::{StreamOfflineV1, StreamOnlineV1},
    Event, EventSubscription, EventsubWebsocketData, Message as EventsubMessage, Payload, ReconnectPayload, SessionData, Transport, WelcomePayload,
};
use twitch_api::helix::eventsub::CreateEventSubSubscription;
use twitch_api::twitch_oauth2::tokens::errors::ValidationError::NotAuthorized;
use twitch_api::types::User;
// use twitch_api::types::eventsub;

use twitch_api::twitch_oauth2::{ClientId, DeviceUserTokenBuilder, RefreshToken, TwitchToken, UserToken};
use twitch_api::{helix, twitch_oauth2, types, HelixClient};

use std::sync::mpsc;
use zerocopy::IntoBytes;

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
                        app.emit("button-pressed", true).ok();
                    }
                    if second == b'5' {
                        println!("MACRO 5");
                        en.key(Key::F15, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F15, Release).unwrap();
                        app.emit("button-pressed", true).ok();
                    }
                    if second == b'6' {
                        println!("MACRO 6");
                        en.key(Key::F16, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F16, Release).unwrap();
                        app.emit("button-pressed", true).ok();
                    }
                    if second == b'7' {
                        println!("MACRO 7");
                        en.key(Key::F17, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F17, Release).unwrap();
                        app.emit("button-pressed", true).ok();
                    }
                    if second == b'8' {
                        println!("MACRO 8");
                        en.key(Key::F18, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F18, Release).unwrap();
                        app.emit("button-pressed", true).ok();
                    }
                    if second == b'9' {
                        println!("MACRO 9");
                        en.key(Key::F19, Press).unwrap();
                        thread::sleep(time::Duration::from_millis(52));
                        en.key(Key::F19, Release).unwrap();
                        app.emit("button-pressed", true).ok();
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

    {
        let init_app = app.clone();
        let init_port = Arc::clone(&port);
        write_deck(init_app, init_port, DeckEvent::StateUpdated);
    }

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

    let tx_clone = tx.clone();
    app.listen_any("clear-deck", move |_e| {
        println!("Clearing deck");

        tx_clone.send(DeckEvent::StateUpdated).unwrap();
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

//SOcket impl
//
async fn refresh_if_expired(token: Arc<AsyncMutex<UserToken>>, helix_client: &HelixClient<'_, reqwest::Client>) {
    let mut lock = token.lock().await;

    if lock.expires_in() >= Duration::from_secs(60) {
        return;
    }
    let client = helix_client.get_client();

    lock.refresh_token(client).await.unwrap();
    // TODO: token refresh logic is left up to the user

    drop(lock);
}

/// action to perform on received message
enum WsAction {
    /// do nothing with the message
    Nothing,
    /// reset the timeout and keep the connection alive
    ResetKeepalive,
    /// kill predecessor and swap the handle
    KillPredecessor,
    /// spawn successor and await death signal
    AssignSuccessor(SocketHandle),
}

async fn subscribe(
    helix_client: &HelixClient<'_, reqwest::Client>,
    session_id: String,
    token: &UserToken,
    subscription: impl EventSubscription + Send,
) -> eyre::Result<()> {
    let transport: Transport = Transport::websocket(session_id);
    let _event_info: CreateEventSubSubscription<_> = helix_client.create_eventsub_subscription(subscription, transport, token).await?;
    Ok(())
}

async fn process_welcome(
    subscribed: &AtomicBool,
    token: &AsyncMutex<UserToken>,
    helix_client: &HelixClient<'_, reqwest::Client>,
    user_id: &types::UserId,
    session: SessionData<'_>,
) -> eyre::Result<()> {
    // if we're already subscribed, don't subscribe again
    if subscribed.load(Ordering::Relaxed) {
        return Ok(());
    }
    let user_token = token.lock().await;

    tokio::try_join!(
        subscribe(
            helix_client,
            session.id.to_string(),
            &user_token,
            StreamOnlineV1::broadcaster_user_id(user_id.clone())
        ),
        subscribe(
            helix_client,
            session.id.to_string(),
            &user_token,
            StreamOfflineV1::broadcaster_user_id(user_id.clone())
        ),
        // subscribe(
        //     helix_client,
        //     session.id.to_string(),
        //     &user_token,
        //     ChannelFollowV2::broadcaster_user_id(user_id.clone())
        // ),
    )?;

    subscribed.store(true, Ordering::Relaxed);
    Ok(())
}

/// Here is where you would handle the events you want to listen to
fn process_payload(event: Event) -> eyre::Result<WsAction> {
    match event {
        Event::ChannelBanV1(Payload { message, .. }) => {
            match message {
                // not needed for websocket
                EventsubMessage::VerificationRequest(_) => unreachable!(),
                EventsubMessage::Revocation() => Err(eyre::eyre!("unexpected subscription revocation")),
                EventsubMessage::Notification(payload) => {
                    // do something useful with the payload
                    tracing::info!(?payload, "got ban event");

                    // new events reset keepalive timeout too
                    Ok(WsAction::ResetKeepalive)
                }
                _ => Ok(WsAction::Nothing),
            }
        }
        Event::ChannelUnbanV1(eventsub::Payload { message, .. }) => {
            match message {
                // not needed for websocket
                EventsubMessage::VerificationRequest(_) => unreachable!(),
                EventsubMessage::Revocation() => Err(eyre::eyre!("unexpected subscription revocation")),
                EventsubMessage::Notification(payload) => {
                    // do something useful with the payload
                    tracing::info!(?payload, "got unban event");

                    // new events reset keepalive timeout too
                    Ok(WsAction::ResetKeepalive)
                }
                _ => Ok(WsAction::Nothing),
            }
        }
        _ => Ok(WsAction::Nothing),
    }
}

struct WebSocketConnection {
    socket: SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    helix_client: &'static HelixClient<'static, reqwest::Client>,
    token: Arc<AsyncMutex<UserToken>>,
    // opts: Arc<crate::Opts>,
    subscribed: Arc<AtomicBool>,
    user_id: Arc<types::UserId>,
    kill_self_tx: UnboundedSender<()>,
}

impl WebSocketConnection {
    async fn receive_message(&mut self) -> eyre::Result<Option<String>> {
        let Some(message) = self.socket.next().await else {
            return Err(eyre::eyre!("websocket stream closed unexpectedly"));
        };
        match message.context("tungstenite error")? {
            WsMessage::Close(frame) => {
                let reason = frame.map(|frame| frame.reason).unwrap_or_default();
                Err(eyre::eyre!("websocket stream closed unexpectedly with reason {reason}"))
            }
            WsMessage::Frame(_) => unreachable!(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => {
                // no need to do anything as tungstenite automatically handles pings for you
                // but refresh the token just in case
                refresh_if_expired(self.token.clone(), self.helix_client).await;
                Ok(None)
            }
            WsMessage::Binary(_) => unimplemented!(),
            WsMessage::Text(payload) => Ok(Some(payload.to_string())),
        }
    }

    async fn process_message(&self, frame: String) -> eyre::Result<WsAction> {
        println!("process_message");
        println!("{:?}", frame);

        let event_data = Event::parse_websocket(&frame).context("parsing error")?;
        match event_data {
            EventsubWebsocketData::Welcome {
                payload: WelcomePayload { session },
                ..
            } => {
                process_welcome(&self.subscribed, &self.token, self.helix_client, &self.user_id, session).await?;
                Ok(WsAction::KillPredecessor)
            }
            EventsubWebsocketData::Reconnect {
                payload: ReconnectPayload { session },
                ..
            } => {
                let url: String = session.reconnect_url.unwrap().into_owned();
                let successor = SocketHandle::spawn(
                    url,
                    self.helix_client,
                    self.kill_self_tx.clone(),
                    self.token.clone(),
                    // self.opts.clone(),
                    self.subscribed.clone(),
                    self.user_id.clone(),
                );
                Ok(WsAction::AssignSuccessor(successor))
            }
            EventsubWebsocketData::Keepalive { .. } => Ok(WsAction::ResetKeepalive),
            EventsubWebsocketData::Revocation { metadata, .. } => {
                eyre::bail!("got revocation: {metadata:?}")
            }
            EventsubWebsocketData::Notification { payload: event, .. } => process_payload(event),
            _ => Ok(WsAction::Nothing),
        }
    }
}

async fn connect_socket(request: impl IntoClientRequest + Unpin) -> Result<SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>, eyre::Error> {
    let config = Some(
        WebSocketConfig::default()
            .max_message_size(Some(64 << 20)) // 64 MiB
            .max_frame_size(Some(16 << 20)) // 16 MiB
            .accept_unmasked_frames(false),
    );
    let socket = tokio_tungstenite::connect_async_with_config(request, config, false)
        .await
        .context("Can't connect to twitch socket")
        .unwrap()
        .0
        .split()
        .1;

    Ok(socket)
}

struct SocketHandle(JoinHandle<eyre::Result<SocketHandle>>);

impl SocketHandle {
    pub fn spawn(
        url: impl IntoClientRequest + Unpin + Send + 'static,
        helix_client: &'static HelixClient<'_, reqwest::Client>,
        kill_predecessor_tx: UnboundedSender<()>,
        token: Arc<AsyncMutex<UserToken>>,
        // opts: Arc<crate::Opts>,
        subscribed: Arc<AtomicBool>,
        user_id: Arc<types::UserId>,
    ) -> Self {
        Self(tokio::spawn(async move {
            println!("Swawning socket");
            let socket = connect_socket(url).await?;
            // If we receive a reconnect message we want to spawn a new connection to twitch.
            // The already existing session should wait on the new session to receive a welcome message before being closed.
            // https://dev.twitch.tv/docs/eventsub/handling-websocket-events/#reconnect-message
            let (kill_self_tx, mut kill_self_rx) = tokio_mpsc::unbounded_channel::<()>();

            let mut connection = WebSocketConnection {
                socket,
                helix_client,
                token,
                // opts,
                subscribed,
                user_id,
                kill_self_tx,
            };

            /// default keepalive duration is 10 seconds
            const WINDOW: u64 = 10;
            let mut timeout: Instant = Instant::now() + Duration::from_secs(WINDOW);
            let mut successor: Option<Self> = None;

            loop {
                tokio::select! {
                    biased;
                    result = kill_self_rx.recv() => {
                        println!("Kill self socket");

                        result.unwrap();
                        let Some(successor) = successor else {
                            // can't receive death signal from successor if it isn't spawned yet
                            unreachable!();
                        };
                        return Ok(successor);
                    }
                    result = connection.receive_message() => if let Some(frame) = result? {
                        let side_effect = connection.process_message(frame).await?;

                        match side_effect {
                            WsAction::Nothing => {}
                            WsAction::ResetKeepalive => timeout = Instant::now() + Duration::from_secs(WINDOW),
                            WsAction::KillPredecessor => {
                                kill_predecessor_tx.send(())?
                            },
                            WsAction::AssignSuccessor(actor_handle) => {
                                successor = Some(actor_handle);
                            },
                        }
                    },
                    _ = tokio::time::sleep_until(timeout) => eyre::bail!("connection timed out"),
                }
            }
        }))
    }

    pub async fn join(self) -> Result<eyre::Result<Self>, JoinError> {
        self.0.await
    }
}

//SOcket impl end
static HELIX_CLIENT: LazyLock<HelixClient<'_, reqwest::Client>> = LazyLock::new(|| HelixClient::default());

async fn get_twitch_data(
    helix_client: &'static HelixClient<'static, reqwest::Client>,
    app: &AppHandle,
    user_id: &str,
    twitch_token_mutex: Arc<AsyncMutex<UserToken>>,
) -> Result<(), eyre::Error> {
    println!("\n⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️Fetch twitch data⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️⬇️");

    let mut twitch_token = twitch_token_mutex.lock().await.clone();
    let state_mutex = app.state::<Mutex<AppState>>();

    let channel = helix_client.get_channel_from_id(user_id, &twitch_token).await.unwrap().unwrap();
    let broadcaster_name = channel.broadcaster_name.as_str();
    let followers = helix_client.get_total_channel_followers(channel.broadcaster_id, &twitch_token).await.unwrap();

    //Refresh token if expires
    if twitch_token.expires_in() <= Duration::from_secs(60) {
        println!("Token is about to expire. Trying to refresh...");
        twitch_token.refresh_token(helix_client).await.context("Refreshing token from polling.")?;
    }

    {
        let mut state = state_mutex.lock().unwrap();

        state.followers = followers;
        app.emit("followers-changed", followers).ok();

        println!("\n==💜== Followers fetched: {}", followers);
    }

    let live_data: Vec<helix::search::Channel> = helix_client.search_channels(broadcaster_name, false, &twitch_token).try_collect().await?;

    {
        let is_live = live_data[0].is_live;
        let mut state = state_mutex.lock().unwrap();

        state.is_live = is_live;
        app.emit("live-changed", is_live).ok();

        println!("\n==🔴== Live fetched: {}", is_live);
    }

    let stream_data: Vec<helix::streams::Stream> = helix_client
        .get_streams_from_ids(&[user_id][..].into(), &twitch_token)
        .try_collect()
        .await
        .unwrap();

    if stream_data.len() != 0 {
        let viewers = stream_data[0].viewer_count as i32;
        let mut state = state_mutex.lock().unwrap();

        state.viewers = viewers;
        app.emit("viewers-changed", viewers).ok();
        println!("\n==😶== Viewers changed: {}", viewers);
    }

    Ok(())
}

#[tauri::command]
async fn auth_twitch(app: AppHandle) {
    println!("\nAuth Twitch.");

    let state_mutex = app.state::<Mutex<AppState>>();
    let store = app.store("store.json").unwrap();
    let twitch_client_id = env::var("TWITCH_CLIENT_ID").expect("No TWITCH_CLIENT_ID in .env file");
    let client_id = twitch_oauth2::ClientId::new(twitch_client_id.clone());

    let access_token_value = store.get("twitch_access_token");

    let reqwest = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

    if access_token_value.is_none() {
        let mut builder = DeviceUserTokenBuilder::new(
            client_id.clone(),
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

    let refresh_token_value = store.get("twitch_refresh_token").unwrap();

    // println!("Refresh token value {:?}\n", refresh_token_value);
    let refresh_token_string: String = serde_json::from_value(refresh_token_value).unwrap();
    let refresh_token = RefreshToken::from_str(refresh_token_string.as_str()).unwrap();

    let twitch_token = UserToken::from_existing_or_refresh_token(&reqwest, access_token, refresh_token, client_id, None)
        .await
        .context("Creating user token from refresh token...")
        .unwrap();

    let helix_client: &'static HelixClient<_> = LazyLock::force(&HELIX_CLIENT);
    {
        let mut state = state_mutex.lock().unwrap();
        state.is_twitch_auth = true;
    }

    app.emit("twitch-auth", true).ok();

    //Connnect to the twitch socket
    let url = twitch_api::TWITCH_EVENTSUB_WEBSOCKET_URL.clone();
    let user_id = Arc::new(twitch_token.user_id.clone());
    let token_mutex = Arc::new(AsyncMutex::new(twitch_token));
    let subscribed = Arc::new(AtomicBool::new(false));

    // get_twitch_data(helix_client, &app, user_id, twitch_token.clone()).await;

    let (dummy_tx, _unused_rx) = tokio_mpsc::unbounded_channel::<()>();

    let mut handle = SocketHandle::spawn(
        url.clone(),
        &helix_client,
        dummy_tx.clone(),
        token_mutex.clone(),
        // opts.clone(),
        subscribed.clone(),
        user_id.clone(),
    );

    let polling_token = token_mutex.clone();
    let polling_user_id = user_id.clone();

    tokio::spawn(async move {
        loop {
            println!("Polling \n");
            get_twitch_data(helix_client, &app, polling_user_id.as_str(), polling_token.clone())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });

    loop {
        handle = match handle.join().await.unwrap() {
            Ok(handle) => handle,
            Err(err) => {
                subscribed.store(false, Ordering::Relaxed);
                tracing::error!("{err}");
                SocketHandle::spawn(
                    url.clone(),
                    helix_client,
                    dummy_tx.clone(),
                    token_mutex.clone(),
                    subscribed.clone(),
                    user_id.clone(),
                )
            }
        }
    }
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

#[tauri::command]
async fn clear_store(app: AppHandle) {
    let store = app.store("store.json").unwrap();
    store.clear();

    app.request_restart();
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
        .invoke_handler(tauri::generate_handler![init_deck, init_discord, auth_twitch, get_state, clear_store])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
