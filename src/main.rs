pub mod wave;
pub mod bit_set;
pub mod acoustic;
pub mod module;

use acoustic::{AcousticSender, AcousticReceiver};
use wave::Wave;


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(44100);
const WAVE_LENGTH: usize = 16;
const SECTION_LEN: usize = 48;
const DATA_PACK_SIZE: usize = 256;

// const BARKER: [bool; 13] = [
//     true, true, true, true, true, false, false,
//     true, true, false, true, false, true
// ];

const BARKER: [bool; 11] = [
    true, true, true, false, false, false,
    true, false, false, true, false
];

// const BARKER: [bool; 7] = [true, true, true, false, false, true, false];

fn compare(receiver: &AcousticReceiver, sender: &AcousticSender, i: u8)
           -> Result<(), Box<dyn std::error::Error>>
{
    let send = [i; DATA_PACK_SIZE / 8];

    sender.send(send)?;

    let recv = receiver.recv()?;

    if send != recv {
        println!("->{:?}", send);
        println!("<-{:?}", recv);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // acoustic::print_hosts();

    let wave = Wave::new(WAVE_LENGTH, std::i16::MAX as usize);

    let receiver = AcousticReceiver::new(wave, SECTION_LEN)?;

    let sender = AcousticSender::new(wave, SECTION_LEN)?;

    for i in 0..=255 {
        compare(&receiver, &sender, i)?;
    }

    // compare(&receiver, &sender, 7)?;
    //
    // compare(&receiver, &sender, 8)?;

    Ok(())
}
