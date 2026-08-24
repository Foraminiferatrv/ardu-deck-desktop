extern crate core;

const BAUDRATE: u32 = 115200;

use core::time;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use enigo::Direction::{Click, Press, Release};
use enigo::Key::N;
use enigo::{Button, Enigo, Key, Keyboard};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serial2::SerialPort;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::http::Method;
use tauri::{http, Manager};

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
    MicroUpdated,
    LiveUpdated,
    ViewersUpdated,
    FollowersUpdate,
    None,
}

#[derive(Clone, Copy, Debug)]
struct State {
    is_micro_on: bool,
    is_live: bool,
    viewers: i32,
    followers: i32,
}

impl State {
    // fn default() -> State {
    //     State {
    //         is_live: false,
    //         is_micro_on: false,
    //         followers: 0,
    //         viewers: 0,
    //     }
    // }

    fn new() -> State {
        State {
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
    fn set_followers(&mut self, new_value: i32) {
        self.followers = new_value;
    }

    fn serialize(&self, event: DeckEvent) -> Vec<u8> {
        let mut message: Vec<&[u8]> = vec![b"*"];
        let viewers = format!("V{:0>3}", self.viewers.to_string());
        let followers = format!("F{:0>3}", self.followers.to_string());

        match event {
            DeckEvent::MicroUpdated => {
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

            _ => {
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
        }

        message.push(b"$");

        message.concat()
    }
}

fn read_deck(state: Arc<Mutex<State>>, port: Arc<Mutex<SerialPort>>, tx: mpsc::Sender<DeckEvent>) {
    let mut read_buffer = [0; 8];
    let mut en = Enigo::new(&enigo::Settings::default()).unwrap();

    // let port = Arc::clone(&state.port);

    loop {
        // println!("Read thread");
        // thread::sleep(time::Duration::from_millis(40));
        thread::sleep(time::Duration::from_millis(60));
        //
        // match read_buffer {
        //     [first, second, third, ..] => {
        //         println!("first {:#?}", first);
        //     }
        // };
        {
            let port = port.lock().unwrap();
            let read = port.read(&mut read_buffer).unwrap_or_else(|_e| 0);

            if read != 0 {
                let [first, second, _third, fourth, ..] = read_buffer;

                if first == b'B' {
                    if second == b'3' {
                        en.key(Key::F13, Click).unwrap();
                        // en.key(Key::F13, Press).unwrap();
                        // thread::sleep(time::Duration::from_millis(60));
                        // en.key(Key::F13, Release).unwrap();

                        let mut state = state.lock().unwrap();
                        if fourth == b'1' {
                            println!("IT'S A MICROPHONE BUTTON: ON");
                            state.set_is_micro_on(true);
                        } else if fourth == b'0' {
                            println!("IT'S A MICROPHONE BUTTON: off");

                            state.set_is_micro_on(false);
                        }
                        tx.send(DeckEvent::MicroUpdated).unwrap();
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
                        // en.key(Key::F19, Click).unwrap();
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

fn write_deck(state: Arc<Mutex<State>>, port: Arc<Mutex<SerialPort>>, event: DeckEvent) {
    // let message = b"*M1L1V204F001$";
    println!("Event2: {:?}", event);

    let state = state.lock().unwrap();
    let message = state.serialize(event);
    let port = port.lock().unwrap();

    println!("Message: {:?}", message);

    for byte in message {
        println!("Write: {}", byte);
        port.write(byte.as_bytes()).expect("Error writing bytes");
        // port.write_all(byte.as_bytes()).expect("Error writing bytes");
        port.flush().unwrap();

        thread::sleep(time::Duration::from_millis(35)); // Delay in write: 20 x 14 = 280ms
    }
}

// Data: Object {"cmd": String("AUTHORIZE"), "data": Object {"code": String("VVL5XYSmTtf2lPYZzsHUSAzTXJpeqO")}, "evt": Null, "nonce": String("null")}
#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthorizeData {
    code: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordAuthorizeResponse {
    cmd: String,

    data: DiscordAuthorizeData,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscordTokenResponse {
    access_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RPCMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DiscordVoiceData {
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    deaf: bool,
}

#[tauri::command]
async fn listen_discord_mic() {
    println!("init discord");

    let client_id = "1541090155040084039";
    let client_secret = "87X0jd_d5nj-GTURHkIvN6tvICIc9LY7";
    let mut access_token: Option<String> = None;

    let mut client = DiscordIpcClient::new(client_id);

    match client.connect() {
        Err(e) => {
            println!("Connecting discord error: {:?}\n", e);
            return ();
        }
        Ok(v) => println!("Connecting discord success: {:?}\n", v),
    };

    // loop {
    // println!("DIS:");

    // match client.set_activity(activity::Activity::new().state("VOICE_STATE_UPDATE")) {
    //     Err(e) => println!("avtivity Error: {:?}", e),
    //     Ok(v) => println!("activity: {:?}", v),
    // }
    //
    //

    let auth_data = serde_json::json!({
      "nonce": "null",
      "args": {
        "client_id": client_id,
        "scopes": ["rpc"]
      },
      "cmd": "AUTHORIZE"
    });

    match client.send(auth_data, 1) {
        Err(e) => println!("auth Error: {:?}\n", e),
        Ok(data) => println!("auth: {:?}\n", data),
    };
    match client.recv() {
        Err(e) => println!("Recv Error: {:?}\n", e),
        Ok((_code, data)) => {
            println!("Data: {}\n", data);
            let de_res: DiscordAuthorizeResponse = serde_json::from_value(data).unwrap();

            let client = reqwest::Client::new();
            // let req = client
            //     .post("https://streamkit.discord.com/overlay/token")
            //     .json(&serde_json::json!({ "code": de_res.data.code }));
            //

            let form = reqwest::multipart::Form::new()
                .text("client_id", client_id)
                .text("grant_type", "authorization_code")
                .text("client_secret", client_secret)
                .text("code", de_res.data.code);

            let req = client
                .post("https://discord.com/api/v10/oauth2/token")
                .multipart(form)
                .header("Content-Type", "application/x-www-form-urlencoded");

            // let req = client
            //     .post(format!(
            //         "https://discord.com/api/v10/oauth2/token?client_id={}&client_secret={}&scope=rpc&code={}&grant_type=authorization_code&redirect_uri=http://localhost",
            //         client_id, client_secret, de_res.data.code
            //     ))
            //     .header("Content-Type", "application/x-www-form-urlencoded");

            // .header("Accept-Encoding", "application/x-www-form-urlencoded");
            // "https://discord.com/api/v10/oauth2/token?client_id={}&client_secret={}&code={}&grant_type=authorization_code&redirect_uri=http://localhost",

            println!("REQUEST: {:?}\n", req);
            let res = req.send().await.unwrap();
            println!("RESPONSE::::: {:?}\n", res);
            // println!("RESPONSE_bytes::::: {:?}\n", res.bytes().await);

            let token = match res.json::<DiscordTokenResponse>().await {
                Err(e) => {
                    println!("json eror xxxxxx {:?}\n", e);

                    DiscordTokenResponse {
                        access_token: String::from("None"),
                    }
                }
                Ok(access_token) => access_token,
            };

            println!("RES++++++ {:?}\n", token);
            // &grant_type=authorization_code
            access_token = Some(token.access_token);
        }
    };

    println!("Token::::::::::::::::  {:?}\n", access_token);

    match access_token {
        None => {
            println!("No token.");
        }

        Some(token) => {
            let authenticate_date = serde_json::json!({
              "nonce": "null",
              "args": {
                  "access_token": token
              },
              "cmd": "AUTHENTICATE"
              // "cmd": "AUTHENTICATE"
            });

            match client.send(authenticate_date, 1) {
                Err(e) => println!("authenticate Error: {:?}\n", e),
                Ok(data) => println!("authenticate: {:?}\n", data),
            };

            match client.recv() {
                Err(e) => println!("Recv Error: {:?}\n", e),
                Ok((code, data)) => {
                    println!("Data: {}\n", data);
                    // let de_res: DiscordAuthorizeResponse = serde_json::from_value(data).unwrap();

                    // access_token = Some(de_res.data.code);
                }
            };

            let settings_req_data = serde_json::json!({
              "nonce": "null",
              "args": {
                  "access_token": token
              },
              // "cmd": "GET_VOICE_SETTINGS"
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
                        println!("data: {:?}\n", data);
                        let msg: RPCMessage = serde_json::from_value(data).unwrap_or_default();
                        let voice_status: DiscordVoiceData = serde_json::from_value(msg.data.unwrap_or_default()).unwrap_or_default();
                        println!("settings: {:?}\n", voice_status);
                        let is_mute = voice_status.mute;
                        println!("IS MUTE!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!: {}\n", is_mute);
                    }
                };
            }
        }
    };
}

#[tauri::command]
async fn init_deck() {
    println!("Init deck");

    let mut raw_port = SerialPort::open("COM3", BAUDRATE).unwrap();
    raw_port.set_read_timeout(Duration::new(0, 0)).unwrap();

    let port: Arc<Mutex<SerialPort>> = Arc::new(Mutex::new(raw_port));
    // SerialPort::

    // let port: Arc<SerialPort> = Arc::new(port);

    let (tx, rx) = mpsc::channel::<DeckEvent>();

    let state = Arc::new(Mutex::new(State::new()));

    let reader_handle = thread::spawn({
        let reader_state = Arc::clone(&state);
        let reader_port = Arc::clone(&port);
        let reader_tx = tx.clone();

        move || read_deck(reader_state, reader_port, reader_tx)
    });

    loop {
        // println!("Write thread");

        let event = match rx.try_recv() {
            Ok(val) => val,
            Err(_) => DeckEvent::None,
        };

        if event != DeckEvent::None {
            println!("Event: {:?}", event);
            let writer_state = Arc::clone(&state);
            let writer_port = Arc::clone(&port);

            write_deck(writer_state, writer_port, event);
            tx.send(DeckEvent::None).unwrap();
        }
    }

    // let _res = ;

    // let _res = reader_handle.join().unwrap();
    //
    // tokio::join!(reader_handle);

    // Ok(())
}

// // #[tokio::main]
// fn main() {
//     if let Err(()) = do_main() {
//         std::process::exit(1);
//     }
// }
//

#[tauri::command]
async fn auth_twitch() {
    println!("Auth Twitch.");

    // let oauth = TwitchOauth::new("your_client_id", "your_client_secret").with_redirect_uri(RedirectUrl::from_str("http://localhost:3000/auth/callback")?);

    // let mut auth_request = oauth.authorization_url();
    // auth_request.scopes_mut().send_chat_message().get_channel_emotes().modify_channel_info();

    // let auth_url = auth_request.url();
    // println!("Visit: {}", auth_url);

    // In your callback handler:
    // let callback: AuthCallback = /* parse from URL */;
    // let token = oauth.exchange_code(callback.code, callback.state).await?;
}

#[tauri::command]
async fn auth_discord() {
    println!("Auth Discord.");
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // let (tx, rx) = mpsc::channel::<TauriDeckEvent>();

    // thread::spawn(|| {
    //     init_deck();
    // });
    //

    tauri::Builder::default()
        // .setup(|app| {
        //     app.manage(state);
        //     Ok(())
        // })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
            println!("a new app instance was opened with {argv:?} and the deep link event was already triggered");
            // when defining deep link schemes at runtime, you must also check `argv` here
        }))
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![init_deck, listen_discord_mic, auth_twitch, auth_discord])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
