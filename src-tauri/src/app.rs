use serde::Serialize;

use crate::deck::DeckEvent;

pub fn serialize_message(state: &AppState, event: DeckEvent) -> Vec<u8> {
    let mut message: Vec<&[u8]> = vec![b"*"];
    let viewers = format!("V{:0>3}", state.viewers.to_string());
    let followers = format!("F{:0>3}", state.followers.to_string());

    match event {
        DeckEvent::MicroUpdated(_) => {
            if state.is_micro_on {
                message.push(b"M1");
            } else {
                message.push(b"M0");
            }
        }

        DeckEvent::LiveUpdated => {
            if state.is_live {
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
            if state.is_micro_on {
                message.push(b"M1");
            } else {
                message.push(b"M0");
            }

            //Live serialize
            if state.is_live {
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
            if state.is_backlight_on {
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

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub struct AppState {
    pub is_discord_auth: bool,
    pub is_twitch_auth: bool,
    pub is_micro_on: bool,
    pub is_live: bool,
    pub is_backlight_on: bool,
    pub viewers: i32,
    pub followers: i64,
}

#[allow(dead_code)]
impl AppState {
    pub fn new() -> AppState {
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

    pub fn serialize(self, event: DeckEvent) -> Vec<u8> {
        serialize_message(&self, event)
    }
}
