pub mod carrier;
pub mod bit_iter;
pub mod acoustic;
pub mod module;
pub mod mac;

#[macro_use]
extern crate lazy_static;

use std::{env, fs::File, io::{Read, Write}};
use crate::{
    acoustic::Athernet,
    mac::{BODY_MAX_SIZE, MacLayer, MacData, mac_unwrap},
    bit_iter::{BitToByteIter, ByteToBitIter},
};


const DATA_PACK_SIZE: usize = 128;

pub type DataPack = [u8; DATA_PACK_SIZE];

const FILE_SIZE: usize = 10000;
const PAYLOAD_SIZE: usize = BODY_MAX_SIZE - 1;
const PACK_NUM: usize = (FILE_SIZE + PAYLOAD_SIZE * 8 - 1) / (PAYLOAD_SIZE * 8);
const BYTE_NUM: usize = (FILE_SIZE + 7) / 8;


pub struct FileRead<T> {
    iter: T,
    count: u8,
    dest: u8,
    mac_layer: MacLayer,
}

impl<T> FileRead<T> {
    pub fn new(iter: T, dest: u8, mac_layer: MacLayer) -> Self { Self { iter, count: 0, dest, mac_layer } }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; PAYLOAD_SIZE];

        ret[0] = self.count;

        self.count += 1;

        let mut size = PAYLOAD_SIZE;

        for (index, item) in ret[1..].iter_mut().enumerate() {
            if let Some(byte) = self.iter.next() {
                *item = byte;
            } else {
                if index == 0 { return None; }
                size = index + 1;
                break;
            }
        }

        Some(self.mac_layer.wrap(self.dest, MacData::DATA, &ret[..size]))
    }
}

enum Command {
    Send(String),
    Recv(String),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();

    args.next();

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

    let mac_layer = MacLayer::new(0b101010);

    let athernet = Athernet::new(mac_layer.clone())?;

    for command in commands {
        match command {
            Command::Send(file) => {
                let send_file = |file| -> Result<_, Box<dyn std::error::Error>> {
                    let file = File::open(file)?;

                    assert_eq!(file.metadata()?.len(), FILE_SIZE as u64);

                    let iter = BitToByteIter::from(file.bytes()
                        .map(|byte| {
                            match byte.unwrap() as char {
                                '0' => false,
                                '1' => true,
                                _ => panic!(),
                            }
                        }));

                    for data_pack in FileRead::new(iter, 0b010101, mac_layer.clone()) {
                        athernet.send(&data_pack)?;
                    }

                    Ok(())
                };

                send_file(file.clone())?;

                send_file(file)?;

                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            Command::Recv(file) => {
                let mut flag = [false; PACK_NUM];
                let mut all_data = [0u8; BYTE_NUM];
                let mut count = 0;

                while count < PACK_NUM {
                    let pack = athernet.recv()?;
                    let (_, data) = mac_unwrap(&pack);

                    let num = data[0] as usize;

                    if !flag[num] {
                        flag[num] = true;

                        let point = num * PAYLOAD_SIZE;

                        all_data[point..point + data.len()].copy_from_slice(data);

                        count += 1;

                        println!("receive {}", num);
                    }
                }

                let data = ByteToBitIter::from(all_data.iter().cloned())
                    .take(FILE_SIZE).map(|bit| if bit { '1' } else { '0' } as u8)
                    .collect::<Box<_>>();

                assert_eq!(data.len(), FILE_SIZE);

                File::create(file)?.write_all(&data).unwrap();
            }
        }
    }

    Ok(())
}
