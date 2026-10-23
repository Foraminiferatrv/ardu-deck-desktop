use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{env, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use crate::AppState;

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
pub async fn init_discord(app: AppHandle) -> Result<(), ()> {
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
