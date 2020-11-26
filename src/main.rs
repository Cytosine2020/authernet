mod utils;
mod athernet;
pub mod wire;

#[macro_use]
extern crate lazy_static;


use std::{env, fs::File, io::{Read, BufReader, Write}};
use crate::{athernet::{MacLayer, mac::{MAC_PAYLOAD_MAX, MacPayload}}, utils::slice_to_le_u64};


pub struct FileRead<T> {
    iter: T,
}

impl<T> FileRead<T> {
    pub fn new(iter: T) -> Self {
        Self { iter }
    }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = MacPayload;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; MAC_PAYLOAD_MAX];
        let mut size = MAC_PAYLOAD_MAX - 1;

        for i in 0..MAC_PAYLOAD_MAX - 1 {
            if let Some(byte) = self.iter.next() {
                ret[i + 1] = byte;
            } else {
                if i == 0 { return None; }
                size = i;
                break;
            }
        }

        ret[0] = size as u8;

        Some(ret)
    }
}

enum Command {
    Send(u8, String),
    Recv(u8, String),
    Ping(u8),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let _ = IPV4Layer::new([192, 168, 0, 1], 4);

    let mut args = env::args();

    args.next();

    let src = args.next().unwrap().parse::<u8>()? & 0b1111;
    let mut commands = Vec::new();
    let mut perf = false;
    let mut wait = 0;

    while let Some(command_) = args.next() {
        let command = command_.as_bytes();

        if command[0] as char != '-' || command.len() != 2 {
            return Err(String::from(
                format!("unknown command: {:?}", command_).to_owned()
            ).into());
        }

        match command[1] as char {
            'e' => perf = true,
            's' => {
                commands.push(Command::Send(
                    args.next().unwrap().parse::<u8>()?, args.next().unwrap(),
                ))
            }
            'r' => {
                commands.push(Command::Recv(
                    args.next().unwrap().parse::<u8>()?, args.next().unwrap(),
                ))
            }
            'p' => commands.push(Command::Ping(args.next().unwrap().parse::<u8>()?)),
            'w' => wait = args.next().unwrap().parse::<u64>()?,
            _ => {
                Err(format!("command {:?} need parameter!", command))?;
            }
        }
    }

    let mut athernet = MacLayer::new(src, perf)?;

    for command in commands {
        match command {
            Command::Send(dest, name) => {
                let file = File::open(name.clone())?;

                let size = file.metadata()?.len();

                println!("sending {:?}, size {}", name, size);

                let mut buffer = [0; MAC_PAYLOAD_MAX];
                buffer[0] = 8;
                buffer[1..9].copy_from_slice(&size.to_le_bytes());

                athernet.send(&buffer, dest)?;

                let iter = BufReader::new(file)
                    .bytes().filter_map(|item| item.ok());

                for data_pack in FileRead::new(iter) {
                    athernet.send(&data_pack, dest)?;
                }
            }
            Command::Recv(dest, name) => {
                let first_pack = athernet.recv(dest)?;

                let size = slice_to_le_u64(&*first_pack);
                let mut count = 0;

                println!("receiving {:?}, size {}", name, size);

                let mut file = File::create(name)?;

                while count < size {
                    let data = athernet.recv(dest)?;

                    file.write_all(&*data)?;
                    count += data.len() as u64;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));

                println!("receive {}", count);
            }
            Command::Ping(dest) => {
                for tag in 0..=255u8 {
                    println!("ping {}: {:?}", tag, athernet.ping(dest)?);
                }
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_secs(wait));

    Ok(())
}
