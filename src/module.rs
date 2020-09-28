use std::collections::VecDeque;
use crate::{
    DATA_PACK_SIZE, SECTION_LEN, WAVE_LENGTH,
    wave::Wave,
    bit_set::{DataPack, BitReceive, BitIter},
};


const PRE_PREAMBLE: [bool; 5] = [false, true, false, true, false];

// const BARKER: [bool; 13] = [
//     true, true, true, true, true, false, false,
//     true, true, false, true, false, true
// ];

const BARKER: [bool; 11] = [
    true, true, true, false, false, false,
    true, false, false, true, false
];

// const BARKER: [bool; 7] = [true, true, true, false, false, true, false];


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
    pub fn new(carrier: Wave, len: usize) -> Self { Self { carrier, len } }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        let preamble = PRE_PREAMBLE.iter().chain(BARKER.iter()).cloned();

        std::iter::empty()
            .chain(bpsk_modulate(preamble, self.carrier.clone(), self.len))
            .chain(bpsk_modulate(BitIter::new(buffer), self.carrier.clone(), self.len))
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
    const WINDOW_EXTRA_SIZE: usize = SECTION_LEN * PRE_PREAMBLE.len();

    const HEADER_THRESHOLD_SCALE: i64 = 1 << 22;
    const MOVING_AVERAGE: i64 = 512;
    const ACTIVE_THRESHOLD: i64 = 512;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
    {
        iter_a.zip(iter_b).map(|(a, b)| a as i64 * b as i64).sum::<i64>()
    }

    fn moving_average(last: i64, new: i64) -> i64 {
        (last * (Self::MOVING_AVERAGE - 1) + new) / Self::MOVING_AVERAGE
    }

    pub fn new(carrier: Wave, len: usize) -> Self {
        let preamble = bpsk_modulate(BARKER.iter().cloned(), carrier.clone(), len)
            .collect::<Vec<_>>();

        Self {
            window: VecDeque::with_capacity(BARKER.len() + Self::WINDOW_EXTRA_SIZE),
            state: DemodulateState::WAITE,
            carrier,
            preamble,
            last_prod: 0,
            moving_average: 0,
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<DataPack> {
        if self.window.len() == self.preamble.len() + Self::WINDOW_EXTRA_SIZE {
            self.window.pop_front();
        }

        self.window.push_back(item);

        self.moving_average = Self::moving_average(self.moving_average, (item as i64).abs());

        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;

        match self.state {
            DemodulateState::WAITE => {
                if self.window.len() >= self.preamble.len() &&
                    self.moving_average > Self::ACTIVE_THRESHOLD {
                    let prod = Self::dot_product(
                        self.window.iter().skip(Self::WINDOW_EXTRA_SIZE).cloned(),
                        self.preamble.iter().cloned(),
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
                    self.window.iter().skip(Self::WINDOW_EXTRA_SIZE).cloned(),
                    self.preamble.iter().cloned(),
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
                            if BARKER.iter().enumerate()
                                .all(|(index, bit)| {
                                    let shift = Self::WINDOW_EXTRA_SIZE - count_copy;

                                    let prod = Self::dot_product(
                                        self.window.iter()
                                            .skip(shift + index * SECTION_LEN).cloned(),
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
            DemodulateState::RECEIVE(ref mut count, ref mut buffer) => {
                *count += 1;

                if *count == SECTION_LEN {
                    let prod = Self::dot_product(
                        self.window.iter().skip(self.window.len() - *count).cloned(),
                        self.carrier.iter(0),
                    );

                    *count = 0;

                    if buffer.push(prod < 0) == DATA_PACK_SIZE {
                        let result = buffer.into_array();

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
