mod websocket;

use eyre::Context;
use futures::TryStreamExt;
use serde_json::json;
use std::{
    env,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::Duration,
};
use tauri::{async_runtime::Mutex as AsyncMutex, AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::mpsc::{self as tokio_mpsc};

use twitch_api::twitch_oauth2::{DeviceUserTokenBuilder, RefreshToken, TwitchToken, UserToken};
use twitch_api::{helix, twitch_oauth2, HelixClient};

use crate::twitch::websocket::SocketHandle;
use crate::AppState;

//SOcket impl end
static HELIX_CLIENT: LazyLock<HelixClient<'_, reqwest::Client>> = LazyLock::new(|| HelixClient::default());

pub async fn get_twitch_data(
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
pub async fn auth_twitch(app: AppHandle) {
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
