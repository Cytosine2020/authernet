use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};


pub struct WaveGen {
    t: u64,
    rate: u64,
    amp: u64,
}

impl WaveGen {
    pub fn new(t: u64, rate: u64, amp: u64) -> Self { Self { t, rate, amp } }

    pub fn calculate(&self) -> i16 {
        ((self.t as f32 * 2. * std::f32::consts::PI / self.rate as f32).sin() * self.amp as f32) as i16
    }

    pub fn get_t(&self) -> &u64 { &self.t }

    pub fn get_t_mut(&mut self) -> &mut u64 { &mut self.t }
}

impl Iterator for WaveGen {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.calculate();
        self.t += 1;
        if self.t >= self.rate { self.t = 0; }
        Some(ret)
    }
}

pub struct Encoder<I, T> {
    iter: I,
    carrier: T,
    tick: u64,
}

impl<I, T> Encoder<I, T> {
    pub fn new(iter: I, carrier: T) -> Self { Self { iter, carrier, tick: 0 } }
}

impl<I, T> Iterator for Encoder<I, T>
    where I: Iterator,
          I::Item: Into<bool>,
          T: Iterator,
{
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(item) => if item.into() { Some(1) } else { Some(0) },
            None => None,
        }
    }
}

pub fn print_hosts() {
    for host in cpal::available_hosts().into_iter().filter_map(|item| cpal::host_from_id(item).ok()) {
        dbg!(host.id());

        for device in host.devices().into_iter().flatten() {
            dbg!(device
                .supported_output_configs().expect("error while querying configs")
                .map(|item| item.with_max_sample_rate())
                .collect::<Vec<_>>()
            );
        }
    }
}

fn main() {
    let device = cpal::default_host().default_output_device().expect("no output device available");

    let supported_config = device
        .supported_output_configs().expect("error while querying configs")
        .next().expect("expected sample format not found")
        .with_max_sample_rate();

    dbg!(&supported_config);

    let config = supported_config.into();

    let mut wave = WaveGen::new(0, 400, (i16::MAX / 4) as u64);

    // let msg = vec![true];

    // let encoder = Encoder::new(msg.iter(), wave);

    let output_stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = wave.next().unwrap() as f32 / i16::MAX as f32;
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {}", err);
        }).unwrap();

    output_stream.play().unwrap();

    let input_stream = device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            for sample in data.iter() {
                println!("{}", *sample);
            }
        },
        |err| {
            eprintln!("an error occurred on the inout audio stream: {}", err);
        }).unwrap();

    input_stream.play().unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
}
