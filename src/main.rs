use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::borrow::Borrow;


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(44100);
const WAVE_LENGTH: usize = 200;
const BARKER: [bool; 13] = [
    true, true, true, true, true, false, false,
    true, true, false, true, false, true
];

#[derive(Copy, Clone)]
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

    pub fn set_t(&mut self, t: usize) { self.t = t % self.rate; }

    pub fn get_rate(&self) -> usize { self.rate }
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

pub fn modulate<I>(iter: I, mut carrier: WaveGen) -> impl Iterator<Item=i16>
    where I: Iterator,
          I::Item: Borrow<bool>,
{
    iter.map(move |bit| {
        let wave_len = carrier.get_rate();
        carrier.set_t(if *bit.borrow() { 0 } else { wave_len / 2 });
        carrier.take(wave_len * 10)
    }).flatten()
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

    let input_config = input_device
        .supported_input_configs().expect("error while querying configs")
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().expect("expected configuration not found")
        .into();

    println!("{:?}: {:#?}", output_device.name(), &output_config);

    println!("{:?}: {:#?}", input_device.name(), &input_config);

    let wave = WaveGen::new(0, WAVE_LENGTH, std::i16::MAX as usize);

    let wave_10 = wave.clone().take(WAVE_LENGTH * 10);

    let mut msg = wave_10.clone()
        .chain(modulate(BARKER.iter(), wave.clone()))
        .chain(wave_10.clone());

    let output_stream = output_device.build_output_stream(
        &output_config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = match msg.next() {
                    Some(item) => item as f32 / std::i16::MAX as f32,
                    None => 0.,
                };
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {}", err);
        }).unwrap();

    let input_stream = input_device.build_input_stream(
        &input_config,
        move |data: &[f32], _| {
            for _sample in data.iter() {
                // println!("{}", *sample);
            }
        },
        |err| {
            eprintln!("an error occurred on the inout audio stream: {}", err);
        }).unwrap();

    output_stream.play().unwrap();

    input_stream.play().unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
}
