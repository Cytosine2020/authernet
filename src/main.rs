pub mod wave;
pub mod bit_set;
pub mod acoustic;
pub mod module;

use std::{
    env, fs::File, cmp::min, mem::size_of,
    io::{Read, Write, BufWriter, BufReader},
};
use core::convert::TryInto;
use crate::{
    wave::Wave,
    bit_set::DataPack,
    acoustic::{AcousticSender, AcousticReceiver},
};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const SECTION_LEN: usize = 192;
const CYCLIC_PREFIX: usize = 0;
const BASE_F: usize = 12;
const CHANNEL: usize = 32;
const DATA_PACK_SIZE: usize = 1024;


pub fn compare(receiver: &AcousticReceiver, sender: &AcousticSender, i: u8)
               -> Result<(), Box<dyn std::error::Error>>
{
    let send = [i; DATA_PACK_SIZE / 8];

    sender.send(send)?;

    let recv = receiver.recv()?;

    if !recv.iter().zip(send.iter()).all(|(a, b)| *a == *b) {
        print!("{:02X} ", i);
        for byte in recv.iter() {
            print!("{:02X}", byte);
        }
        println!();
    } else {
        println!("{:02X}", i);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // acoustic::print_hosts();

    let wave = Wave::new(SECTION_LEN, std::i16::MAX as usize, BASE_F, CHANNEL);

    let receiver = AcousticReceiver::new(&wave)?;

    let sender = AcousticSender::new(&wave)?;

    for i in 0..=255 {
        compare(&receiver, &sender, i)?;
    }

    // for i in 0..=255 {
    //     let buf = receiver.recv()?;
    //
    //     if buf != [i; DATA_PACK_SIZE / 8] {
    //         println!("{} {:?}", i, buf);
    //     } else {
    //         println!("{}", i);
    //     }
    // }

    // for i in 0..=255 {
    //     sender.send([i; DATA_PACK_SIZE / 8])?;
    // }
    //
    // std::thread::sleep(std::time::Duration::from_secs(45));

    // let args = env::args().collect::<Vec<_>>();
    //
    // if args.len() != 3 { panic!("accept only two arguments!") }
    //
    // if args[1] == "-s" {
    //     let sender = AcousticSender::new(&wave)?;
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
    //     let receiver = AcousticReceiver::new(&wave)?;
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
