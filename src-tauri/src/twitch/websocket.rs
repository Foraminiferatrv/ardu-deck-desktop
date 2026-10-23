use twitch_api::{types, HelixClient};

use eyre::Context;

use futures::stream::{SplitStream, StreamExt};

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::async_runtime::Mutex as AsyncMutex;
use tokio::{
    sync::mpsc::{self as tokio_mpsc, UnboundedSender},
    task::{JoinError, JoinHandle},
    time::Instant,
};
use twitch_api::twitch_oauth2::{TwitchToken, UserToken};

use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message as WsMessage},
    MaybeTlsStream, WebSocketStream,
};

use twitch_api::eventsub::{
    self,
    stream::{StreamOfflineV1, StreamOnlineV1},
    Event, EventSubscription, EventsubWebsocketData, Message as EventsubMessage, Payload, ReconnectPayload, SessionData, Transport, WelcomePayload,
};
use twitch_api::helix::eventsub::CreateEventSubSubscription;

async fn refresh_if_twitch_token_expired(token: Arc<AsyncMutex<UserToken>>, helix_client: &HelixClient<'_, reqwest::Client>) {
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
                refresh_if_twitch_token_expired(self.token.clone(), self.helix_client).await;
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

pub struct SocketHandle(JoinHandle<eyre::Result<SocketHandle>>);

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
