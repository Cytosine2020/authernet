use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(44100);
const WAVE_LENGTH: usize = 200;

pub struct WaveGen {
    t: usize,
    rate: usize,
    amp: usize,
}

impl WaveGen {
    pub fn new(t: usize, rate: usize, amp: usize) -> Self { Self { t, rate, amp } }

    pub fn calculate(&self) -> i16 {
        let value = (self.t as f32 * 2. * std::f32::consts::PI / self.rate as f32).sin();
        (value * self.amp as f32) as i16
    }
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

pub struct Encoder<I> {
    iter: I,
    carrier: WaveGen,
    tick: usize,
}

impl<I> Encoder<I> {
    const SECTION: usize = WAVE_LENGTH * 10;

    pub fn new(iter: I, carrier: WaveGen) -> Self { Self { iter, carrier, tick: Self::SECTION } }
}

impl<I> Iterator for Encoder<I>
    where I: Iterator,
          I::Item: Into<bool>,
{
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.tick < Self::SECTION {
            let ret = self.carrier.next().unwrap();
            self.tick += 1;
            Some(ret)
        } else {
            match self.iter.next() {
                Some(item) => {
                    let shift = if item.into() { 0 } else { WAVE_LENGTH / 2 };

                    self.carrier = WaveGen::new(shift, WAVE_LENGTH, std::i16::MAX as usize);

                    let ret = self.carrier.next().unwrap();
                    self.tick = 1;
                    Some(ret)
                }
                None => None,
            }
        }
    }
}

pub fn print_hosts() {
    for host in cpal::available_hosts().into_iter()
        .filter_map(|item| cpal::host_from_id(item).ok()) {
        println!("Host: {:?}", host.id());

        for device in host.devices().into_iter().flatten() {
            println!("Device name: {:?}", device.name());

            for config in device
                .supported_output_configs().expect("error while querying configs")
                .map(|item| item.with_max_sample_rate()) {
                println!("output: {:#?}", config);
            }

            for config in device
                .supported_input_configs().expect("error while querying configs")
                .map(|item| item.with_max_sample_rate()) {
                println!("input: {:#?}", config);
            }
        }
    }
}

fn main() {
    let output_device = cpal::default_host()
        .default_output_device().expect("no output device available");

    let input_device = cpal::default_host()
        .default_input_device().expect("no input device available");

    let output_config = output_device
        .supported_output_configs().expect("error while querying configs")
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().expect("expected configuration not found")
        .into();

    println!("{:?}: {:#?}", output_device.name(), &output_config);

    let input_config = input_device
        .supported_input_configs().expect("error while querying configs")
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().expect("expected configuration not found")
        .into();

    println!("{:?}: {:#?}", input_device.name(), &input_config);

    let mut wave = WaveGen::new(0, WAVE_LENGTH, std::i16::MAX as usize);

    let msg = vec![true, true, true, true, true, true];

    let mut encoder = Encoder::new(msg.into_iter(), wave);

    let output_stream = output_device.build_output_stream(
        &output_config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = match encoder.next() {
                    Some(item) => item as f32 / std::i16::MAX as f32,
                    None => 0.,
                };
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {}", err);
        }).unwrap();

    output_stream.play().unwrap();

    let input_stream = input_device.build_input_stream(
        &input_config,
        move |data: &[f32], _| {
            for sample in data.iter() {
                // println!("{}", *sample);
            }
        },
        |err| {
            eprintln!("an error occurred on the inout audio stream: {}", err);
        }).unwrap();

    input_stream.play().unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
}
