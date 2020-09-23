use std::collections::VecDeque;
use crate::{
    DATA_PACK_SIZE, BARKER, SECTION_LEN, WAVE_LENGTH,
    bit_set::{DataPack, BitReceive, BitIter},
};


#[derive(Clone)]
pub struct Wave {
    wave: Vec<i16>,
}

impl Wave {
    pub fn calculate(rate: usize, amp: usize, t: usize) -> i16 {
        ((t as f32 * 2. * std::f32::consts::PI / rate as f32).sin() * amp as f32) as i16
    }

    pub fn new(rate: usize, amp: usize) -> Self {
        let wave = (0..rate).into_iter()
            .map(|i| Self::calculate(rate, amp, i))
            .collect::<Vec<_>>();

        Self { wave }
    }

    pub fn get_rate(&self) -> usize { self.wave.len() }

    pub fn iter(&self, t: usize) -> impl Iterator<Item=i16> {
        self.wave.clone().into_iter().cycle().skip(t % self.get_rate())
    }
}

pub fn bpsk_modulate<I>(iter: I, carrier: Wave, len: usize) -> impl Iterator<Item=i16>
    where I: Iterator<Item=bool>,
{
    iter.map(move |bit| {
        carrier.iter(bit as usize * carrier.get_rate() / 2).take(len)
    }).flatten()
}

pub struct Modulator {
    carrier: Wave,
    len: usize,
}

impl Modulator {
    pub fn new(carrier: &Wave, len: usize) -> Self { Self { carrier: carrier.clone(), len } }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        let iter = [false, true, false, true, false].iter()
            .chain(BARKER.iter()).cloned()
            .chain(BitIter::new(buffer));

        bpsk_modulate(iter, self.carrier.clone(), self.len)
    }
}

enum DemodulateState {
    WAITE,
    MATCH(usize, i64),
    RECEIVE(usize, BitReceive),
}

pub struct Demodulator {
    window: VecDeque<i16>,
    state: DemodulateState,
    carrier: Wave,
    preamble: Vec<i16>,
    last_prod: i64,
    moving_average: i64,
}

impl Demodulator {
    const HEADER_THRESHOLD_SCALE: i64 = (1 << 21) * 3;
    const MOVING_AVERAGE: i64 = 512;
    const ACTIVE_THRESHOLD: i64 = 512;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
    {
        iter_a.zip(iter_b).map(|(a, b)| a as i64 * b as i64).sum::<i64>()
    }

    fn moving_average(last: i64, new: i64, constant: i64) -> i64 {
        (last * (constant - 1) + new) / constant
    }

    pub fn new(preamble: Vec<i16>, carrier: &Wave, len: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(preamble.len() * len),
            state: DemodulateState::WAITE,
            carrier: carrier.clone(),
            preamble,
            last_prod: 0,
            moving_average: 1024,
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<DataPack> {
        if self.window.len() == self.preamble.len() {
            self.window.pop_front();
        }

        self.window.push_back(item);

        self.moving_average = Self::moving_average(
            self.moving_average, (item as i64).abs(), Self::MOVING_AVERAGE,
        );

        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;

        match self.state {
            DemodulateState::WAITE => {
                if self.window.len() == self.preamble.len() &&
                    self.moving_average > Self::ACTIVE_THRESHOLD {
                    let prod = Self::dot_product(
                        self.window.iter().cloned(), self.preamble.iter().cloned(),
                    );

                    if prod > threshold && self.last_prod > prod {
                        // print!("{} {} {} ", item, threshold, prod);

                        self.state = DemodulateState::MATCH(1, self.last_prod);
                    }

                    self.last_prod = prod;
                } else {
                    self.last_prod = 0;
                }
            }
            DemodulateState::MATCH(ref mut count, ref mut last) => {
                let prod = Self::dot_product(
                    self.window.iter().cloned(), self.preamble.iter().cloned(),
                );

                let last_prod = self.last_prod;

                self.last_prod = prod;

                if *count >= WAVE_LENGTH * 2 {
                    self.state = DemodulateState::WAITE;
                } else {
                    *count += 1;

                    if last_prod > prod && *count >= WAVE_LENGTH {
                        // print!("{} {} {} ", item, threshold, prod);

                        let count_copy = *count;

                        self.state = if last_prod < *last {
                            if BARKER.iter().enumerate().skip(1)
                                .all(|(index, bit)| {
                                    let prod = Self::dot_product(
                                        self.window.iter()
                                            .skip(index * SECTION_LEN - count_copy).cloned(),
                                        self.carrier.iter(0).take(SECTION_LEN),
                                    );

                                    *bit == (prod < 0)
                                }) {
                                // println!("match");

                                self.last_prod = 0;

                                DemodulateState::RECEIVE(count_copy, BitReceive::new())
                            } else {
                                // println!("preamble decode failed");

                                DemodulateState::WAITE
                            }
                        } else {
                            DemodulateState::MATCH(1, self.last_prod)
                        };
                    }
                }
            }
            DemodulateState::RECEIVE(
                ref mut wave_count, ref mut data_buffer,
            ) => {
                *wave_count += 1;

                if *wave_count == SECTION_LEN {
                    let prod = Self::dot_product(
                        self.window.iter().skip(self.window.len() - *wave_count).cloned(),
                        self.carrier.iter(0),
                    );

                    *wave_count = 0;

                    if data_buffer.push(prod < 0) == DATA_PACK_SIZE {
                        let result = data_buffer.into_array();

                        self.state = DemodulateState::WAITE;

                        self.window.drain(..self.window.len() - SECTION_LEN);

                        return Some(result);
                    }
                }
            }
        }

        // eprintln!("{}\t{}", threshold, self.last_prod);

        None
    }
}
