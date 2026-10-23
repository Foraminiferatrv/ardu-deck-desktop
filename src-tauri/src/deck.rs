use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard,
};
use serial2::SerialPort;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use tauri::{AppHandle, Emitter, Listener, Manager};

use zerocopy::IntoBytes;

use crate::AppState;

pub const BAUDRATE: u32 = 115200;

#[allow(dead_code)]
#[derive(Debug)]
pub enum DeckButton {
    B3,
    B4,
    B5,
    B6,
    B7,
    B8,
    B9,
}

#[allow(dead_code)]
#[derive(PartialEq, Debug)]
pub enum DeckEvent {
    StateUpdated,
    MicroUpdated(bool),
    LiveUpdated,
    ViewersUpdated,
    FollowersUpdate,
    BacklightUpdate(bool),
    DiscordAuthenticated,
    None,
}

pub fn read_deck(app: AppHandle, port: Arc<Mutex<SerialPort>>) {
    let mut read_buffer = [0; 8];
    let mut en = Enigo::new(&enigo::Settings::default()).unwrap();
    let state_mutex = app.state::<Mutex<AppState>>();

    loop {
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

pub fn write_deck(app: AppHandle, port: Arc<Mutex<SerialPort>>, event: DeckEvent) {
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
pub async fn init_deck(app: AppHandle) -> Result<(), ()> {
    println!("Init deck");

    let mut raw_port = SerialPort::open("COM3", BAUDRATE).unwrap();
    raw_port.set_read_timeout(time::Duration::new(0, 0)).unwrap();

    let port: Arc<Mutex<SerialPort>> = Arc::new(Mutex::new(raw_port));

    {
        let init_app = app.clone();
        let init_port = Arc::clone(&port);
        write_deck(init_app, init_port, DeckEvent::StateUpdated);
    }

    let (tx, rx) = mpsc::channel::<DeckEvent>();

    let read_app = app.clone();

    thread::spawn({
        let reader_port = Arc::clone(&port);
        // let reader_tx = tx.clone();

        move || read_deck(read_app, reader_port)
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
    app.listen_any("backlight-changed", move |e| {
        let data = e.payload();

        let _res = &tx_clone
            .send(if data == "true" {
                DeckEvent::BacklightUpdate(true)
            } else {
                DeckEvent::BacklightUpdate(false)
            })
            .unwrap();
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
