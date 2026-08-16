extern crate core;

const BAUDRATE: u32 = 115200;

use ascii::{AsAsciiStr, ToAsciiChar};
use core::time;
use serial2::SerialPort;
use std::io::{IoSlice, Read, Write};
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use zerocopy::{FromBytes, IntoBytes, Usize};

struct State {
    is_micro_on: bool,
    is_live: bool,
    viewers: i32,
    followers: i32,
}

impl State {
    fn default() -> State {
        State {
            is_live: false,
            is_micro_on: false,
            followers: 0,
            viewers: 0,
        }
    }
}

// static STATE: Arc<Mutex<State>> = Arc::new(Mutex::new(State {
//     is_live: false,
//     is_micro_on: false,
//     followers: 0,
//     viewers: 0,
// }));

fn read_deck(state: Arc<State>, port: Arc<SerialPort>) -> Result<(), ()> {
    let mut read_buffer = [0; 8];
    let mut state = state.clone();
    loop {
        // match read_buffer {
        //     [first, second, third, ..] => {
        //         println!("first {:#?}", first);
        //     }
        // };
        let read = port.read(&mut read_buffer).unwrap_or_else(|_e| 0);

        if read != 0 {
            println!("Read from deck: {:?}", read_buffer);
            println!("Buff {:?}", read_buffer.as_ascii_str().unwrap());

            let [first, second, third, fourth, ..] = read_buffer;
            println!(
                "first: {:?}, second: {:?}, third: {:?}, fourth: {:?}",
                first.to_ascii_char().unwrap(),
                second.to_ascii_char().unwrap(),
                third.to_ascii_char().unwrap(),
                fourth.to_ascii_char().unwrap()
            );

            if first == b'B' {
                if second == b'3' {
                    if fourth == b'1' {
                        println!("IT'S A MICROPHONE BUTTON: ON");
                        state.is_micro_on = true;
                    } else if fourth == b'0' {
                        println!("IT'S A MICROPHONE BUTTON: off");
                        state.is_micro_on = false;
                    }
                }
            }
        };
        thread::sleep(time::Duration::from_millis(60));
    }
}

//1.Serialize state
//2. Listen to state chagne

fn serialize_state<'a>(state: &'a State) -> Vec<u8> {
    // let state_lock = STATE.clone();

    // let state = state_lock.lock().unwrap();

    let mut message: Vec<&[u8]> = vec![b"*"];
    let viewers = format!("V{:0>3}", state.viewers.to_string());
    let followers = format!("F{:0>3}", state.followers.to_string());

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

    message.push(b"$");

    message.concat()
}

fn write_deck(state: &State, port: Arc<SerialPort>) -> Result<(), ()> {
    // let MESSAGE = b"*M1L1V204F001$";
    // let MESSAGE_2 = b"*M0L0V000F111$";
    let mut swi = false;

    loop {
        let message = serialize_state(state);
        // let message = message_vec;

        // println!("WRITE THREAD: {:?}", message);
        for byte in message {
            // println!("writing: {:?}", byte);

            port.write(byte.as_bytes()).unwrap();
            port.flush().unwrap();

            thread::sleep(time::Duration::from_millis(30));
            // thread::sleep(time::Duration::from_millis(400));
        }

        swi = !swi;
    }
}

fn do_main() -> Result<(), ()> {
    let mut port = SerialPort::open("COM3", BAUDRATE).unwrap();
    port.set_read_timeout(Duration::new(0, 0)).unwrap();

    let mut state = Arc::new(State {
        is_micro_on: true,
        is_live: true,
        viewers: 20,
        followers: 321,
    });

    let mut read_state = state.clone();
    // state = State {
    //     is_micro_on: true,
    //     is_live: true,
    //     viewers: 20,
    //     followers: 321,
    // };

    let port: Arc<SerialPort> = Arc::new(port);

    let reader_port = port.clone();

    println!("Spawning read thread:");
    let reader_handle = thread::spawn(move || read_deck(read_state, reader_port));

    let _res = write_deck(&state, port);

    let _res = reader_handle.join().unwrap();

    Ok(())
}

fn main() {
    if let Err(()) = do_main() {
        std::process::exit(1);
    }
}
