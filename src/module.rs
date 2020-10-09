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


pub fn bpsk_modulate<I>(iter: I, carrier: Wave) -> impl Iterator<Item=i16>
    where I: Iterator<Item=bool>,
{
    iter.map(move |bit| {
        carrier.iter().map(move |item| if bit { item } else { -item }).take(SECTION_LEN)
    }).flatten()
}

pub struct Modulator {
    carrier: Wave,
}

impl Modulator {
    pub fn new(carrier: Wave) -> Self { Self { carrier } }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        let preamble = PRE_PREAMBLE.iter().chain(BARKER.iter()).cloned();

        std::iter::empty()
            .chain(bpsk_modulate(preamble, self.carrier.clone()))
            .chain(bpsk_modulate(BitIter::new(buffer), self.carrier.clone()))
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

    const HEADER_THRESHOLD_SCALE: i64 = (1 << 20) * 3;
    const MOVING_AVERAGE: i64 = 512;
    const ACTIVE_THRESHOLD: i64 = 256;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
    {
        iter_a.zip(iter_b).map(|(a, b)| a as i64 * b as i64).sum::<i64>()
    }

    fn preamble_product(&self) -> i64 {
        Self::dot_product(
            self.window.iter().skip(self.window.len() - self.preamble.len()).cloned(),
            self.preamble.iter().cloned(),
        )
    }

    fn section_product(&self, offset: usize) -> i64 {
        Self::dot_product(
            self.window.iter().skip(offset + WAVE_LENGTH).cloned(),
            self.carrier.iter().take(SECTION_LEN - WAVE_LENGTH),
        )
    }

    fn moving_average(last: i64, new: i64) -> i64 {
        (last * (Self::MOVING_AVERAGE - 1) + new) / Self::MOVING_AVERAGE
    }

    pub fn new(carrier: Wave) -> Self {
        let carrier_clone = carrier.clone();

        let preamble = BARKER.iter().cloned().map(move |bit| {
            carrier_clone.iter().map(move |item| if bit { item } else { -item })
                .take(WAVE_LENGTH).enumerate()
                .map(|(index, value)| {
                    (index * value as usize / WAVE_LENGTH) as i16
                }).chain(carrier_clone.iter()
                .map(move |item| if bit { item } else { -item })
                .take(SECTION_LEN - WAVE_LENGTH))
        }).flatten().collect::<Vec<_>>();

        // let preamble = BARKER.iter().cloned().map(move |bit| {
        //     std::iter::repeat(0i16).take(WAVE_LENGTH).chain(
        //         carrier_clone.iter(bit as usize * rate / 2).take(len - WAVE_LENGTH))
        // }).flatten().collect();

        Self {
            window: VecDeque::with_capacity(preamble.len() + Self::WINDOW_EXTRA_SIZE),
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
                    let prod = self.preamble_product();

                    if prod > threshold && self.last_prod > prod {
                        // print!("{} {} {} ", item, threshold, prod);

                        self.state = DemodulateState::MATCH(1, self.last_prod);
                    }

                    self.last_prod = prod;
                } else {
                    self.last_prod = 0;
                }
            }
            DemodulateState::MATCH(mut count, last) => {
                let prod = self.preamble_product();

                let last_prod = self.last_prod;

                self.last_prod = prod;

                self.state = if count >= WAVE_LENGTH * 2 {
                    DemodulateState::WAITE
                } else {
                    count += 1;

                    if last_prod > prod && count >= WAVE_LENGTH {
                        // print!("{} {} {} ", item, threshold, prod);

                        if last_prod < last {
                            if BARKER.len() - 1 <= BARKER.iter()
                                .enumerate().map(|(index, bit)| {
                                let shift = self.window.len() - self.preamble.len() - count;

                                let prod = self.section_product(shift + index * SECTION_LEN);

                                if *bit == (prod > 0) { 1 } else { 0 }
                            }).sum::<usize>() {
                                // println!("match {} {}", self.moving_average, last_prod);

                                self.last_prod = 0;

                                DemodulateState::RECEIVE(count, BitReceive::new())
                            } else {
                                // println!("preamble decode failed {}", match_count);

                                DemodulateState::WAITE
                            }
                        } else {
                            DemodulateState::MATCH(1, self.last_prod)
                        }
                    } else {
                        DemodulateState::MATCH(count, last)
                    }
                }
            }
            DemodulateState::RECEIVE(mut count, mut buffer) => {
                count += 1;

                self.state = if count == SECTION_LEN {
                    let prod = self.section_product(self.window.len() - SECTION_LEN);

                    if buffer.push(prod > 0) == DATA_PACK_SIZE {
                        let result = buffer.into_array();

                        self.state = DemodulateState::WAITE;

                        self.window.drain(..self.window.len() - SECTION_LEN);

                        return Some(result);
                    }

                    DemodulateState::RECEIVE(0, buffer)
                } else {
                    DemodulateState::RECEIVE(count, buffer)
                }
            }
        }

        // eprintln!("{}\t{}", threshold, self.last_prod);

        None
    }
}
