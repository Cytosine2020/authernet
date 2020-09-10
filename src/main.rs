use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};


pub struct WaveGen {
    t: u64,
    t0: u64,
    rate: u64,
}

impl WaveGen {
    pub fn new(t0: u64, rate: u64) -> Self { Self { t: 0, t0, rate } }

    pub fn calculate(&self) -> f32 {
        ((self.t - self.t0) as f32 * 2f32 * std::f32::consts::PI / self.rate as f32).sin()
    }
}

impl Iterator for WaveGen {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = (self.calculate() * i16::max_value() as f32) as i16;
        self.t += 1;
        if self.t > self.rate { self.t -= self.rate; }
        Some(ret)
    }
}

// pub struct Encoder<I, T> {
//     iter: I,
//     carrier: T,
// }
//
// impl<I, T> Encoder<I, T> {
//     pub fn new(iter: I, carrier: T) -> Self { Self { iter, carrier } }
// }
//
// impl<I, T> Iterator for Encoder<I, T>
//     where I: Iterator,
//           I::Item: Into<bool>,
//           T: Iterator,
// {
//     type Item = i32;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         match self.iter.next() {
//             Some(item) => if item.into() { Some(1) } else { Some(0) },
//             None => None,
//         }
//     }
// }


fn main() {
    let device = cpal::default_host().default_output_device().expect("no output device available");

    let supported_config = device
        .supported_output_configs().expect("error while querying configs")
        .filter(|conf| conf.sample_format() == SampleFormat::F32)
        .next().expect("expected sample format not found")
        .with_max_sample_rate();

    let config = supported_config.into();

    let mut wave = WaveGen::new(0, 10);

    // for item in wave {
    //     println!("{}", item)
    // }

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = wave.next().expect("") as f32;
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {}", err)
        }).unwrap();

    stream.play().unwrap();
}
