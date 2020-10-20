pub mod wave;
pub mod bit_iter;
pub mod acoustic;
pub mod module;
pub mod crc;

#[macro_use]
extern crate lazy_static;

use std::{env, fs::File};
use crate::{
    wave::Wave,
    acoustic::{AcousticSender, AcousticReceiver},
    crc::{PAYLOAD_SIZE, FileRead, crc_unwrap},
    bit_iter::{BitToByteIter, ByteToBitIter},
};
use std::io::{Read, Write};


const DATA_PACK_SIZE: usize = 32;

pub type DataPack = [u8; DATA_PACK_SIZE];

const FILE_SIZE: usize = 10000;
const PACK_NUM: usize = (FILE_SIZE + PAYLOAD_SIZE * 8 - 1) / (PAYLOAD_SIZE * 8);
const BYTE_NUM: usize = (FILE_SIZE + 7) / 8;


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wave = Wave::new();

    let args = env::args().collect::<Box<_>>();

    if args.len() != 3 { panic!("accept only two arguments!") }

    if args[1] == "-s" {
        let sender = AcousticSender::new(&wave)?;

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
        let receiver = AcousticReceiver::new(&wave)?;

        let mut flag = [false; PACK_NUM];
        let mut all_data = [0u8; BYTE_NUM];
        let mut count = 0;

        loop {
            if let Some(data) = crc_unwrap(&receiver.recv()?) {
                let size = data[0] as usize;
                let num = data[1] as usize;

                if !flag[num] {
                    flag[num] = true;

                    let point = num * PAYLOAD_SIZE;

                    all_data[point..point + size - 3].copy_from_slice(&data[2..size - 1]);

                    count += 1;

                    println!("receive {}", num);

                    if count == PACK_NUM {
                        let data = ByteToBitIter::from(all_data.iter().cloned())
                            .take(FILE_SIZE)
                            .map(|bit| bit as u8 + '0' as u8).
                            collect::<Box<[u8]>>();

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
