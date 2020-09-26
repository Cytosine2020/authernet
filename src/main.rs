pub mod bit_set;
pub mod acoustic;
pub mod module;

use std::{
    env, fs::File, cmp::min, mem::size_of,
    io::{Read, Write, BufWriter, BufReader},
};
use acoustic::{AcousticSender, AcousticReceiver};
use module::Wave;
use crate::bit_set::DataPack;
use core::convert::TryInto;


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const WAVE_LENGTH: usize = 16;
const SECTION_LEN: usize = 64;
const DATA_PACK_SIZE: usize = 256;


pub fn compare(receiver: &AcousticReceiver, sender: &AcousticSender, i: u8)
               -> Result<(), Box<dyn std::error::Error>>
{
    let send = [i; DATA_PACK_SIZE / 8];

    sender.send(send)?;

    let recv = receiver.recv()?;

    if send != recv {
        println!("-> {:?}", send);
        println!("<- {:?}", recv);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let args = env::args().collect::<Vec<_>>();
    //
    // if args.len() != 3 { panic!("accept only two arguments!") }

    // acoustic::print_hosts();

    let wave = Wave::new(WAVE_LENGTH, std::i16::MAX as usize);

    let receiver = AcousticReceiver::new(&wave, SECTION_LEN)?;

    let sender = AcousticSender::new(&wave, SECTION_LEN)?;

    for i in 0..=255 {
        compare(&receiver, &sender, i)?;
    }

    // for i in 0..=255 {
    //     let buf = receiver.recv()?;
    //
    //     if buf != [i; DATA_PACK_SIZE / 8] {
    //         println!("{} {:?}", i, buf);
    //     }
    // }

    // if args[1] == "-s" {
    //     let sender = AcousticSender::new(&wave, SECTION_LEN)?;
    //
    //     let file = File::open(args[2].clone())?;
    //
    //     let mut size = file.metadata()?.len() as u64;
    //
    //     println!("sending file {:?} with size {}", args[2], size);
    //
    //     let mut buf_read = BufReader::new(file);
    //
    //     let mut buf: DataPack = [0; DATA_PACK_SIZE / 8];
    //
    //     let first_size = min(buf.len() as u64, size + size_of::<u64>() as u64) as usize;
    //
    //     buf[..size_of::<i64>()].copy_from_slice(&size.to_le_bytes());
    //
    //     buf_read.read_exact(&mut buf[size_of::<u64>()..first_size])?;
    //
    //     sender.send(buf)?;
    //
    //     size -= (first_size - size_of::<u64>()) as u64;
    //
    //     while size > buf.len() as u64 {
    //         buf_read.read_exact(&mut buf)?;
    //
    //         sender.send(buf)?;
    //
    //         size -= buf.len() as u64;
    //     }
    //
    //     if size > 0 {
    //         buf_read.read_exact(&mut buf[..size as usize])?;
    //
    //         sender.send(buf)?;
    //     }
    //
    //     std::thread::sleep(std::time::Duration::from_secs(90));
    // } else if args[1] == "-r" {
    //     let receiver = AcousticReceiver::new(&wave, SECTION_LEN)?;
    //
    //     let file = File::create(args[2].clone())?;
    //
    //     let mut buf_writer = BufWriter::new(file);
    //
    //     let mut buf = receiver.recv()?;
    //
    //     let mut size = u64::from_le_bytes(buf[..size_of::<u64>()].try_into()?);
    //
    //     println!("receiving file {:?} with size {}", args[2], size);
    //
    //     let first_size = min(buf.len() as u64, size + size_of::<u64>() as u64) as usize;
    //
    //     if buf_writer.write(&buf[size_of::<u64>()..first_size])? != first_size - size_of::<u64>() {
    //         panic!();
    //     }
    //
    //     size -= (first_size - size_of::<u64>()) as u64;
    //
    //     while size > buf.len() as u64 {
    //         buf = receiver.recv()?;
    //
    //         if buf_writer.write(&buf)? != buf.len() {
    //             panic!();
    //         }
    //
    //         size -= buf.len() as u64;
    //     }
    //
    //     if size > 0 {
    //         buf = receiver.recv()?;
    //
    //         if buf_writer.write(&buf[..size as usize])? != size as usize {
    //             panic!();
    //         }
    //     }
    // } else {
    //     panic!("unknown command: {}", args[1]);
    // }

    Ok(())
}
