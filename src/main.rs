pub mod acoustic;
pub mod module;
pub mod mac;

#[macro_use]
extern crate lazy_static;

use std::{env, fs::File, io::{Read, BufReader, Write}};
use crate::{acoustic::Athernet, mac::{BODY_MAX_SIZE, MacLayer, MacData, DataPack, mac_unwrap}};


pub struct FileRead<T> {
    iter: T,
    mac_layer: MacLayer,
}

impl<T> FileRead<T> {
    pub fn new(iter: T, mac_layer: MacLayer) -> Self {
        Self { iter, mac_layer }
    }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; BODY_MAX_SIZE];
        let mut size = BODY_MAX_SIZE;

        for i in 0..BODY_MAX_SIZE {
            if let Some(byte) = self.iter.next() {
                ret[i] = byte;
            } else {
                if i == 0 { return None; }
                size = i;
                break;
            }
        }

        Some(self.mac_layer.wrap(MacData::DATA, &ret[..size]))
    }
}

enum Command {
    Send(String),
    Recv(String),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();

    args.next();

    let src = args.next().unwrap().parse::<u8>()?;
    let dest = args.next().unwrap().parse::<u8>()?;
    let mut commands = Vec::new();

    loop {
        if let Some(command_) = args.next() {
            let command = command_.as_bytes();

            if command[0] as char != '-' || command.len() != 2 {
                return Err(String::from(
                    format!("unknown command: {:?}", command_).to_owned()
                ).into());
            }

            if let Some(arg) = args.next() {
                match command[1] as char {
                    's' => commands.push(Command::Send(arg)),
                    'r' => commands.push(Command::Recv(arg)),
                    _ => return Err(String::from(
                        format!("unknown command: {:?}", command_).to_owned()
                    ).into()),
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    let mac_layer = MacLayer::new(src, dest);

    let athernet = Athernet::new(mac_layer.clone())?;

    for command in commands {
        match command {
            Command::Send(name) => {
                let file = File::open(name.clone())?;

                let size = file.metadata()?.len();

                println!("sending {:?}, size {}", name, size);

                athernet.send(&mac_layer.wrap(MacData::DATA, &size.to_le_bytes()))?;

                let iter = BufReader::new(file)
                    .bytes().filter_map(|item| item.ok());

                for data_pack in FileRead::new(iter, mac_layer.clone()) {
                    athernet.send(&data_pack)?;
                }
            }
            Command::Recv(name) => {
                let first_pack = athernet.recv()?;
                let (_, first_data) = mac_unwrap(&first_pack);

                let mut size_buffer = [0u8; 8];
                size_buffer.copy_from_slice(first_data);

                let size = u64::from_le_bytes(size_buffer);
                let mut count = 0;

                println!("receiving {:?}, size {}", name, size);

                let mut file = File::create(name)?;

                while count < size {
                    let pack = athernet.recv()?;
                    let (_, data) = mac_unwrap(&pack);

                    file.write_all(data)?;

                    println!("receive {}", data.len());

                    count += data.len() as u64;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_secs(10));

    Ok(())
}
