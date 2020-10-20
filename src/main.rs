pub mod wave;
pub mod bit_iter;
pub mod acoustic;
pub mod module;
pub mod mac;

#[macro_use]
extern crate lazy_static;

use std::{env, fs::File, io::{Read, Write}};
use crate::{
    acoustic::{AcousticSender, AcousticReceiver},
    mac::{CRC_SIZE, BODY_INDEX, BODY_MAX_SIZE, SIZE_INDEX, crc_generate, crc_unwrap},
    bit_iter::{BitToByteIter, ByteToBitIter},
};


const DATA_PACK_SIZE: usize = 32;

pub type DataPack = [u8; DATA_PACK_SIZE];

const FILE_SIZE: usize = 10000;
const PAYLOAD_SIZE: usize = BODY_MAX_SIZE - 1;
const PACK_NUM: usize = (FILE_SIZE + PAYLOAD_SIZE * 8 - 1) / (PAYLOAD_SIZE * 8);
const BYTE_NUM: usize = (FILE_SIZE + 7) / 8;


pub struct FileRead<T> {
    iter: T,
    count: u8,
}

impl<T> FileRead<T> {
    pub fn new(iter: T) -> Self { Self { iter, count: 0 } }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; DATA_PACK_SIZE];

        ret[BODY_INDEX] = self.count;

        self.count += 1;

        let mut size = DATA_PACK_SIZE;

        for i in 0..PAYLOAD_SIZE {
            if let Some(byte) = self.iter.next() {
                ret[i + 1 + BODY_INDEX] = byte;
            } else {
                if i == 0 { return None; }
                size = i + 1 + BODY_INDEX + CRC_SIZE;
                break;
            }
        }

        ret[SIZE_INDEX] = size as u8;
        crc_generate(&mut ret);

        Some(ret)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Box<_>>();

    if args.len() != 3 { panic!("accept only two arguments!") }

    if args[1] == "-s" {
        let sender = AcousticSender::new()?;

        let send_file = || -> Result<_, Box<dyn std::error::Error>> {
            let file = File::open(args[2].clone())?;

            assert_eq!(file.metadata()?.len(), FILE_SIZE as u64);

            let iter = BitToByteIter::from(file.bytes().map(|byte| {
                match byte.unwrap() as char {
                    '0' => false,
                    '1' => true,
                    _ => panic!(),
                }
            }));

            for data_pack in FileRead::new(iter) {
                sender.send(&data_pack)?;
            }

            Ok(())
        };

        send_file()?;

        send_file()?;

        std::thread::sleep(std::time::Duration::from_secs(15));
    } else if args[1] == "-r" {
        let receiver = AcousticReceiver::new()?;

        let mut flag = [false; PACK_NUM];
        let mut all_data = [0u8; BYTE_NUM];
        let mut count = 0;

        loop {
            if let Some(data) = crc_unwrap(&receiver.recv()?) {
                let num = data[0] as usize;

                if !flag[num] {
                    flag[num] = true;

                    let point = num * PAYLOAD_SIZE;

                    all_data[point..point + data.len() - CRC_SIZE]
                        .copy_from_slice(&data[BODY_INDEX..]);

                    count += 1;

                    println!("receive {}", num);

                    if count == PACK_NUM {
                        let data = ByteToBitIter::from(all_data.iter().cloned())
                            .take(FILE_SIZE).map(|bit| if bit { '1' } else { '0' } as u8)
                            .collect::<Box<[u8]>>();

                        assert_eq!(data.len(), FILE_SIZE);

                        File::create(args[2].clone())?.write_all(&data).unwrap();
                        break;
                    }
                }
            } else {
                println!("crc fail!");
            }
        }
    } else {
        panic!("unknown command: {}", args[1]);
    }

    Ok(())
}
