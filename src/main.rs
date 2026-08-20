extern crate core;

const BAUDRATE: u32 = 115200;

use core::time;
use serial2::SerialPort;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use std::sync::mpsc;
use zerocopy::IntoBytes;

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

    fn serialize(&self) -> Vec<u8> {
        let mut message: Vec<&[u8]> = vec![b"*"];
        let viewers = format!("V{:0>3}", self.viewers.to_string());
        let followers = format!("F{:0>3}", self.followers.to_string());

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

        message.push(b"$");

        message.concat()
    }
}

fn read_deck(state: Arc<Mutex<State>>, port: Arc<Mutex<SerialPort>>, tx: mpsc::Sender<bool>) {
    let mut read_buffer = [0; 8];
    // let port = Arc::clone(&state.port);

    loop {
        println!("Read thread");
        // match read_buffer {
        //     [first, second, third, ..] => {
        //         println!("first {:#?}", first);
        //     }
        // };
        {
            let port = port.lock().unwrap();
            let read = port.read(&mut read_buffer).unwrap_or_else(|_e| 0);

            if read != 0 {
                // println!("Read from deck: {:?}", read_buffer);
                // println!("Buff {:?}", read_buffer.as_ascii_str().unwrap());

                let [first, second, _third, fourth, ..] = read_buffer;

                // println!(
                //     "first: {:?}, second: {:?}, third: {:?}, fourth: {:?}",
                //     first.to_ascii_char().unwrap(),
                //     second.to_ascii_char().unwrap(),
                //     third.to_ascii_char().unwrap(),
                //     fourth.to_ascii_char().unwrap()
                // );

                if first == b'B' {
                    if second == b'3' {
                        let mut state = state.lock().unwrap();
                        if fourth == b'1' {
                            println!("IT'S A MICROPHONE BUTTON: ON");
                            state.set_is_micro_on(true);
                        } else if fourth == b'0' {
                            println!("IT'S A MICROPHONE BUTTON: off");

                            state.set_is_micro_on(false);
                        }
                        tx.send(true).unwrap();
                    }

                    if second == b'4' {
                        println!("MACRO 4")
                    }
                    if second == b'5' {
                        println!("MACRO 5")
                    }
                    if second == b'6' {
                        println!("MACRO 6")
                    }
                    if second == b'7' {
                        println!("MACRO 7")
                    }
                    if second == b'8' {
                        println!("MACRO 8")
                    }
                    if second == b'9' {
                        println!("MACRO 9")
                    }
                }
            };
            thread::sleep(time::Duration::from_millis(60));
        }
    }
}

//1.Serialize state
//2. Listen to state chagne
fn write_deck(state: Arc<Mutex<State>>, port: Arc<Mutex<SerialPort>>) {
    // let message = b"*M1L1V204F001$";

    let state = state.lock().unwrap();
    let message = state.serialize();
    let port = port.lock().unwrap();

    // println!("Message: {:?}", message);
    for byte in message {
        println!("Write: {}", byte);
        port.write(byte.as_bytes()).expect("Error writing bytes");
        // port.write_all(byte.as_bytes()).expect("Error writing bytes");
        port.flush().unwrap();

        thread::sleep(time::Duration::from_millis(20)); // Delay in write: 20 x 14 = 280ms
        // thread::sleep(time::Duration::from_millis(30));
    }
    // }
    // thread::sleep(time::Duration::from_millis(2000));
    // }
}

fn do_main() -> Result<(), ()> {
    let mut raw_port = SerialPort::open("COM3", BAUDRATE).unwrap();
    raw_port.set_read_timeout(Duration::new(0, 0)).unwrap();

    let port: Arc<Mutex<SerialPort>> = Arc::new(Mutex::new(raw_port));
    // SerialPort::

    // let port: Arc<SerialPort> = Arc::new(port);

    let (tx, rx) = mpsc::channel::<bool>();

    let state = Arc::new(Mutex::new(State::new()));
    // let port = state.port;

    // let state = Arc::new(Mutex::new(State {
    //     is_micro_on: true,
    //     is_live: true,
    //     viewers: 20,
    //     followers: 321,
    // }));

    // let write_state = state.clone();
    // state = State {
    //     is_micro_on: true,
    //     is_live: true,
    //     viewers: 20,
    //     followers: 321,
    // };

    // let writer_port = port.clone();

    // let reader_handle = tokio::spawn(async move { read_deck(read_state, reader_port).await });
    // let writer_handle = tokio::spawn(async move { write_deck(write_state, writer_port).await });

    let reader_handle = thread::spawn({
        let reader_state = Arc::clone(&state);
        let reader_port = Arc::clone(&port);
        let reader_tx = tx.clone();

        move || read_deck(reader_state, reader_port, reader_tx)
    });

    loop {
        // println!("Write thread");

        let res = match rx.try_recv() {
            Ok(val) => val,
            Err(_) => false,
        };

        if res {
            let writer_state = Arc::clone(&state);
            let writer_port = Arc::clone(&port);

            write_deck(writer_state, writer_port);
            tx.send(false).unwrap();
        }
    }

    // let _res = ;

    // let _res = reader_handle.join().unwrap();
    //
    // tokio::join!(reader_handle);

    // Ok(())
}

// #[tokio::main]
fn main() {
    if let Err(()) = do_main() {
        std::process::exit(1);
    }
}
