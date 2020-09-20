use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::borrow::Borrow;
use std::collections::VecDeque;


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(44100);
const WAVE_LENGTH: usize = 16;
const SECTION_LEN: usize = 48;
const DATA_PACK: usize = 128;

// const BARKER: [bool; 13] = [
//     true, true, true, true, true, false, false,
//     true, true, false, true, false, true
// ];

const BARKER: [bool; 11] = [
    true, true, true, false, false, false,
    true, false, false, true, false
];

// const BARKER: [bool; 7] = [true, true, true, false, false, true, false];

#[derive(Copy, Clone)]
pub struct BitReceive {
    inner: [u8; DATA_PACK / 8],
    count: usize,
}

impl BitReceive {
    pub fn new() -> Self { Self { inner: [0; DATA_PACK / 8], count: 0 } }

    pub fn push(&mut self, bit: bool) -> usize {
        self.inner[self.count / 8] |= (bit as u8) << (self.count % 8);
        self.count += 1;
        self.count
    }

    pub fn into_array(self) -> [u8; DATA_PACK / 8] { self.inner }
}

#[derive(Copy, Clone)]
pub struct WaveGen {
    t: usize,
    rate: usize,
    amp: usize,
}

impl WaveGen {
    pub fn calculate(t: usize, rate: usize, amp: usize) -> i16 {
        ((t as f32 * 2. * std::f32::consts::PI / rate as f32).sin() * amp as f32) as i16
    }

    pub fn new(t: usize, rate: usize, amp: usize) -> Self { Self { t, rate, amp } }

    pub fn set_t(&mut self, t: usize) { self.t = t % self.rate; }

    pub fn get_rate(&self) -> usize { self.rate }

    pub fn iter(&self) -> impl Iterator<Item=i16> {
        let copy_a = self.clone();
        let copy_b = self.clone();

        std::iter::repeat((copy_a.t..copy_a.rate).chain(0..copy_a.t)).flatten()
            .map(move |t| Self::calculate(t, copy_b.rate, copy_b.amp))
    }
}

pub struct Chunk<I> {
    iter: I,
}

impl<I> Chunk<I> {
    pub fn new(iter: I) -> Self { Self { iter } }
}

impl<I> Iterator for Chunk<I>
    where I: Iterator,
          I::Item: Borrow<bool>,
{
    type Item = (I::Item, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(item_a) => {
                match self.iter.next() {
                    Some(item_b) => Some((item_a, item_b)),
                    None => panic!(),
                }
            }
            None => None,
        }
    }
}

pub fn bpsk_modulate<I>(iter: I, mut carrier: WaveGen, len: usize) -> impl Iterator<Item=i16>
    where I: Iterator,
          I::Item: Borrow<bool>,
{
    iter.map(move |bit| {
        carrier.set_t(*bit.borrow() as usize * carrier.get_rate() / 2);
        carrier.iter().take(len)
    }).flatten()
}

pub fn qpsk_modulate<I>(iter: I, mut carrier: WaveGen, len: usize) -> impl Iterator<Item=i16>
    where I: Iterator,
          I::Item: Borrow<bool>,
{
    Chunk::new(iter).map(move |(bit_a, bit_b)| {
        let wave_len = carrier.get_rate();
        carrier.set_t((*bit_a.borrow() as usize * 2 + *bit_b.borrow() as usize) * wave_len / 4);
        carrier.iter().take(len)
    }).flatten()
}

enum DemodulateState {
    WAITE,
    MATCH(usize, i64),
    RECEIVE(Vec<i16>, BitReceive),
}

pub struct Demodulator {
    window: VecDeque<i16>,
    state: DemodulateState,
    carrier: WaveGen,
    preamble: Vec<i16>,
    last_prod: i64,
    moving_average: i64,
}

impl Demodulator {
    pub fn new<I>(preamble_: I, carrier: WaveGen, len: usize) -> Self
        where I: Iterator,
              I::Item: Borrow<bool>,
    {
        let preamble = preamble_
            .map(|item| *item.borrow()).collect::<Vec<_>>();

        Self {
            window: VecDeque::with_capacity(preamble.len() * len),
            state: DemodulateState::WAITE,
            carrier,
            preamble: bpsk_modulate(preamble.iter(), carrier, len).collect::<Vec<_>>(),
            last_prod: 0,
            moving_average: 1024,
        }
    }

    pub fn receive(&mut self, item: i16) -> Option<[u8; DATA_PACK / 8]> {
        if let DemodulateState::RECEIVE(
            ref mut wave_buffer,
            ref mut data_buffer,
        ) = self.state {
            wave_buffer.push(item);

            if wave_buffer.len() == SECTION_LEN {
                let mut wave = self.carrier;

                let (i, _) = (0..2).map(|i| {
                    wave.set_t(i * wave.get_rate() / 2);

                    let prod = wave_buffer.iter()
                        .zip(wave.iter())
                        .map(|(a, b)| *a as i64 * b as i64)
                        .sum::<i64>();

                    (i, prod)
                }).max_by_key(|(_, prod)| prod.clone()).unwrap();

                wave_buffer.clear();

                if data_buffer.push(i == 1) == DATA_PACK {
                    let result = data_buffer.into_array();

                    self.state = DemodulateState::WAITE;

                    return Some(result);
                }
            }
        }

        None
    }

    pub fn push_back(&mut self, item: i16) -> Option<[u8; DATA_PACK / 8]> {
        if self.window.len() == self.preamble.len() {
            self.window.pop_front();
        }

        self.window.push_back(item);

        let ret = self.receive(item);

        let threshold = self.moving_average * (1 << 23);

        if self.window.len() == self.preamble.len() {
            let prod = self.window.iter()
                .zip(self.preamble.iter())
                .map(|(a, b)| *a as i64 * *b as i64)
                .sum::<i64>();

            self.moving_average = (self.moving_average * 511 + item.abs() as i64) / 512;

            let flag = self.moving_average > 1024 && self.last_prod > prod;

            match self.state {
                DemodulateState::WAITE => {
                    if flag && prod > threshold {
                        self.state = DemodulateState::MATCH(1, self.last_prod);

                        eprint!("{} {} {} ", item, threshold, prod);
                    }
                }
                DemodulateState::MATCH(ref mut count, ref mut last) => {
                    if *count >= WAVE_LENGTH * 2 {
                        self.state = DemodulateState::WAITE;
                    } else {
                        *count += 1;

                        if flag && *count >= WAVE_LENGTH {
                            eprint!("{} {} {} ", item, threshold, prod);

                            if self.last_prod < *last {
                                let count_copy = *count;

                                let wave_buffer = Vec::with_capacity(SECTION_LEN);
                                let data_buffer = BitReceive::new();

                                self.state = DemodulateState::RECEIVE(wave_buffer, data_buffer);

                                for i in self.window.len() - count_copy..self.window.len() {
                                    self.receive(self.window[i]);
                                }
                            } else {
                                *count = 1;
                                *last = self.last_prod;
                            }
                        }
                    }
                }
                _ => {}
            }

            self.last_prod = prod;

            println!("{}\t{}\t{}", item, threshold, prod);
        } else {
            println!("{}\t{}\t{}", item, threshold, 0);
        }

        ret
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
    // print_hosts();

    let output_device = cpal::default_host()
        .default_output_device().expect("no output device available");

    let input_device = cpal::default_host()
        .default_input_device().expect("no input device available");

    let output_config = output_device
        .supported_output_configs().expect("error while querying configs")
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().expect("expected configuration not found");

    let input_config = input_device
        .supported_input_configs().expect("error while querying configs")
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().expect("expected configuration not found");

    let output_channel = output_config.channels() as usize;

    // println!("{:?}: {:#?}", output_device.name(), &output_config);
    //
    // println!("{:?}: {:#?}", input_device.name(), &input_config);

    let wave = WaveGen::new(0, WAVE_LENGTH, std::i16::MAX as usize);

    // let msg = std::iter::empty()
    //     .chain(bpsk_modulate(std::iter::repeat([true].iter().cloned()).flatten().take(256), wave, SECTION_LEN))
    //     .chain(bpsk_modulate(BARKER.iter().cloned(), wave, SECTION_LEN))
    //     .chain(qpsk_modulate(std::iter::repeat([true, true, true, false].iter().cloned()).flatten().take(DATA_PACK), wave, SECTION_LEN))
    //     .chain(bpsk_modulate(BARKER.iter().cloned(), wave, SECTION_LEN))
    //     .chain(qpsk_modulate(std::iter::repeat([false, false, false, true].iter().cloned()).flatten().take(DATA_PACK), wave, SECTION_LEN));

    let msg = bpsk_modulate(
        std::iter::empty()
            .chain(std::iter::repeat([true].iter().cloned()).flatten().take(256))
            .chain(BARKER.iter().cloned())
            .chain(std::iter::repeat([true, true, true, false, false, false, false, false].iter().cloned()).flatten().take(DATA_PACK))
            .chain(BARKER.iter().cloned())
            .chain(std::iter::repeat([false, false, false, true, false, false, false, false].iter().cloned()).flatten().take(DATA_PACK)),
        wave, SECTION_LEN);

    let mut msg_channel = msg
        .map(move |item| {
            std::iter::once(item).chain(std::iter::repeat(0).take(output_channel - 1))
        }).flatten();

    let mut demodulator = Demodulator::new(BARKER.iter(), wave, SECTION_LEN);

    // for i in msg {
    //     demodulator.push_back(i);
    // }

    let output_stream = output_device.build_output_stream(
        &output_config.into(),
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = match msg_channel.next() {
                    Some(item) => item as f32 / std::i16::MAX as f32,
                    None => 0.,
                };
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {:?}", err);
        }).unwrap();

    let input_stream = input_device.build_input_stream(
        &input_config.into(),
        move |data: &[f32], _| {
            for sample in data.iter() {
                let val = (*sample * std::i16::MAX as f32) as i16;

                if let Some(buffer) = demodulator.push_back(val) {
                    eprintln!("{:?}", buffer);
                }
            }
        },
        |err| {
            eprintln!("an error occurred on the inout audio stream: {:?}", err);
        }).unwrap();

    input_stream.play().unwrap();

    output_stream.play().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1000));
}
