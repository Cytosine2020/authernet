pub mod wave;
pub mod bit_set;
pub mod acoustic;
pub mod module;
pub mod crc_add;

#[macro_use]
extern crate lazy_static;

use std::{env, fs::File};
use crate::{
    wave::Wave,
    acoustic::{AcousticSender, AcousticReceiver},
    crc_add::{FileRead, FileWrite},
};


const FILE_SIZE: usize = 10000;

const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const SECTION_LEN: usize = 96;
const CYCLIC_PREFIX: usize = 0;
const BASE_F: usize = 8;
const CHANNEL: usize = 8;
const DATA_PACK_SIZE: usize = 256;


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wave = Wave::new(SECTION_LEN, std::i16::MAX as usize, BASE_F, CHANNEL);

    let args = env::args().collect::<Vec<_>>();

    if args.len() != 3 { panic!("accept only two arguments!") }

    if args[1] == "-s" {
        let sender = AcousticSender::new(&wave)?;

        let file = File::open(args[2].clone())?;

        assert_eq!(file.metadata()?.len(), FILE_SIZE as u64);

        let read_in = FileRead::new(file);

        for i in read_in {
            sender.send(i)?;
        }

        std::thread::sleep(std::time::Duration::from_secs(90));
    } else if args[1] == "-r" {
        let receiver = AcousticReceiver::new(&wave)?;

        let file = File::create(args[2].clone())?;

        let mut write_data = FileWrite::new(file);

        while write_data.count != 0 {
            let buf = receiver.recv()?;
            write_data.write_in(buf);
        }

        write_data.write_allin();
    } else {
        panic!("unknown command: {}", args[1]);
    }

    Ok(())
}
