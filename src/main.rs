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
const PACK_NUM: usize = (FILE_SIZE + (BODY_MAX_SIZE - 1) * 8 - 1) / ((BODY_MAX_SIZE - 1) * 8);
const BYTE_NUM: usize = (FILE_SIZE + 7) / 8;


pub struct FileRead<T> {
    iter: T,
    count: u8,
    dest: u8,
    mac_layer: MacLayer,
}

impl<T> FileRead<T> {
    pub fn new(iter: T, dest: u8, mac_layer: MacLayer) -> Self {
        Self { iter, count: 0, dest, mac_layer }
    }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; BODY_MAX_SIZE];

        ret[0] = self.count;

        self.count += 1;

        let mut size = BODY_MAX_SIZE;

        for i in 1..BODY_MAX_SIZE {
            if let Some(byte) = self.iter.next() {
                ret[i] = byte;
            } else {
                if i == 1 { return None; }
                size = i;
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
    let mut src_ = Vec::new();
    let mut dest_ = Vec::new();

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
                    'c' => src_.push(arg.parse::<u8>()?),
                    'd' => dest_.push(arg.parse::<u8>()?),
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

    if src_.len() != 1 || src_[0] > MacData::MAC_MASK {
        Err("no src or multiple src or src to big")?
    }

    if dest_.len() != 1 || dest_[0] > MacData::MAC_MASK {
        Err("no dest or multiple dest or dest too big")?
    }

    let src = src_[0];
    let dest = dest_[0];

    let mac_layer = MacLayer::new(src);

    let athernet = Athernet::new(mac_layer.clone())?;

    for command in commands {
        match command {
            Command::Send(name) => {
                let file = File::open(name.clone())?;

                assert_eq!(file.metadata()?.len(), FILE_SIZE as u64);

                let iter = BitToByteIter::from(file.bytes()
                    .map(|byte| {
                        match byte.unwrap() as char {
                            '0' => false,
                            '1' => true,
                            _ => panic!(),
                        }
                    }));

                for data_pack in FileRead::new(iter, dest, mac_layer.clone()) {
                    athernet.send(&data_pack)?;
                }

                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Command::Recv(name) => {
                let mut flag = [false; PACK_NUM];
                let mut all_data = [0u8; BYTE_NUM];
                let mut count = 0;

                while count < PACK_NUM {
                    let pack = athernet.recv()?;
                    let (_, data) = mac_unwrap(&pack);

                    let num = data[0] as usize;

                    if !flag[num] {
                        flag[num] = true;

                        let point = num * (BODY_MAX_SIZE - 1);

                        all_data[point..point + data.len() - 1].copy_from_slice(&data[1..]);

                        count += 1;

                        println!("receive {}", num);
                    }
                }

                let data = ByteToBitIter::from(all_data.iter().cloned())
                    .take(FILE_SIZE).map(|bit| if bit { '1' } else { '0' } as u8)
                    .collect::<Box<_>>();

                assert_eq!(data.len(), FILE_SIZE);

                File::create(name)?.write_all(&data).unwrap();

                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }

    Ok(())
}
