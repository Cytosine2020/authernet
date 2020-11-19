pub mod rtaudio;
pub mod athernet;
pub mod module;
pub mod mac;

#[macro_use]
extern crate lazy_static;


use std::{env, fs::File, io::{Read, BufReader, Write}};
use crate::mac::{DATA_PACK_MAX, DataPack, MacLayer};

fn data_pack_unwrap(data_pack: &DataPack) -> &[u8] {
    &data_pack[1..][..data_pack[0] as usize]
}

pub struct FileRead<T> {
    iter: T,
}

impl<T> FileRead<T> {
    pub fn new(iter: T) -> Self {
        Self { iter }
    }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; DATA_PACK_MAX];
        let mut size = DATA_PACK_MAX - 1;

        for i in 0..DATA_PACK_MAX - 1 {
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
    Send(String),
    Recv(String),
    Ping,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();

    args.next();

    let src = args.next().unwrap().parse::<u8>()? & 0b1111;
    let dest = args.next().unwrap().parse::<u8>()? & 0b1111;
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
            'p' => commands.push(Command::Ping),
            'e' => perf = true,
            _ => {
                if let Some(arg) = args.next() {
                    match command[1] as char {
                        's' => commands.push(Command::Send(arg)),
                        'r' => commands.push(Command::Recv(arg)),
                        'w' => wait = arg.parse::<u64>()?,
                        _ => return Err(String::from(
                            format!("unknown command: {:?}", command_).to_owned()
                        ).into()),
                    }
                } else {
                    Err(format!("command {:?} need parameter!", command))?;
                }
            }
        }
    }

    let mut athernet = MacLayer::new(src, dest, perf)?;

    for command in commands {
        match command {
            Command::Send(name) => {
                let file = File::open(name.clone())?;

                let size = file.metadata()?.len();

                println!("sending {:?}, size {}", name, size);

                let mut buffer = [0; DATA_PACK_MAX];
                buffer[0] = 8;
                buffer[1..9].copy_from_slice(&size.to_le_bytes());

                athernet.send(&buffer)?;

                let iter = BufReader::new(file)
                    .bytes().filter_map(|item| item.ok());

                for data_pack in FileRead::new(iter) {
                    athernet.send(&data_pack)?;
                }
            }
            Command::Recv(name) => {
                let first_pack = athernet.recv()?;
                let first_data = data_pack_unwrap(&first_pack);

                let mut size_buffer = [0u8; 8];
                size_buffer.copy_from_slice(first_data);

                let size = u64::from_le_bytes(size_buffer);
                let mut count = 0;

                println!("receiving {:?}, size {}", name, size);

                let mut file = File::create(name)?;

                while count < size {
                    let pack = athernet.recv()?;
                    let data = data_pack_unwrap(&pack);

                    // println!("receive {}", data.len());

                    file.write_all(data)?;
                    count += data.len() as u64;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));

                println!("receive {}", count);
            }
            Command::Ping => {
                for tag in 0..=255u8 {
                    println!("ping {}: {:?}", tag, athernet.ping()?);
                }
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_secs(wait));

    Ok(())
}
