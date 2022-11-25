use crossterm::event::{poll, read, Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub enum Event<I> {
    Input(I),
    Quit,
    Tick,
}

pub struct Events {
    recv: mpsc::Receiver<Event<KeyEvent>>,
    _input_handle: thread::JoinHandle<()>,
}

impl Events {
    pub fn new() -> Events {
        let (sender, reciever) = mpsc::channel();
        let _input_handle = {
            let sender = sender.clone();
            thread::spawn(move || loop {
                let event = match poll(Duration::from_millis(3000))
                    .expect("Docs say this will never return Err")
                {
                    true => match read() {
                        Ok(TermEvent::Key(event)) => match (event.code, event.modifiers) {
                            (KeyCode::Char('q'), KeyModifiers::NONE)
                            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => Event::Quit,
                            _ => Event::Input(event),
                        },
                        Ok(_) => Event::Tick,
                        Err(_) => Event::Tick,
                    },
                    false => Event::Tick,
                };
                sender.send(event).unwrap();
            })
        };
        Events {
            recv: reciever,
            _input_handle,
        }
    }

    pub fn next(&self) -> Event<KeyEvent> {
        match self.recv.recv() {
            Ok(e) => e,
            Err(_) => {
                println!("Events channel disconnected. Exiting.");
                Event::Quit
            }
        }
    }
}
