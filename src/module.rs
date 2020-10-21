use std::collections::VecDeque;
use crate::{
    DATA_PACK_SIZE, DataPack,
    wave::{SECTION_LEN, BASE_F, CHANNEL, Synthesizer, carrier},
    bit_iter::ByteToBitIter,
    mac::{SIZE_INDEX, SIZE_SIZE},
};


const CYCLIC_PREFIX: usize = 0;

const PRE_PREAMBLE: [bool; 5] = [false, true, false, true, false];

const BARKER: [bool; 11] = [
    true, true, true, false, false, false,
    true, false, false, true, false
];


fn bpsk_modulate<I>(iter: I) -> impl Iterator<Item=i16>
    where I: Iterator<Item=bool>,
{
    iter.map(move |bit| {
        carrier(0, SECTION_LEN - CYCLIC_PREFIX)
            .map(move |item| if bit { item } else { -item })
            .take(SECTION_LEN + CYCLIC_PREFIX)
    }).flatten()
}

fn ofdm_modulate<I>(mut iter: I) -> impl Iterator<Item=i16>
    where I: Iterator<Item=bool>,
{
    let size = iter.size_hint().1.unwrap() / CHANNEL;

    (0..size).map(move |_| {
        let channels = (0..CHANNEL)
            .map(|i| {
                let bit = iter.next().unwrap();
                carrier(i, SECTION_LEN - CYCLIC_PREFIX)
                    .map(move |item| if bit { item } else { -item })
            });

        Synthesizer::new(channels).take(SECTION_LEN + CYCLIC_PREFIX)
    }).flatten()
}

pub struct Modulator {}

impl Modulator {
    pub fn new() -> Self { Self {} }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        let preamble = PRE_PREAMBLE.iter().chain(BARKER.iter()).cloned();

        let iter = ByteToBitIter::from(
            (0..buffer[SIZE_INDEX] as usize).map(move |index| buffer[index])
        );

        bpsk_modulate(preamble).chain(ofdm_modulate(iter))
    }
}

enum DemodulateState {
    WAITE,
    MATCH(usize, i64),
    RECEIVE(usize, BitReceive),
}

#[derive(Copy, Clone)]
pub struct BitReceive {
    inner: DataPack,
    count: usize,
}

impl BitReceive {
    #[inline]
    pub fn new() -> Self { Self { inner: [0; DATA_PACK_SIZE], count: 0 } }

    #[inline]
    pub fn push(&mut self, bit: bool) -> Option<Result<DataPack, ()>> {
        self.inner[self.count / 8] |= (bit as u8) << (self.count % 8);
        self.count += 1;

        if self.count <= (SIZE_INDEX + SIZE_SIZE) * 8 {
            None
        } else {
            if self.inner[SIZE_INDEX] as usize > DATA_PACK_SIZE {
                Some(Err(()))
            } else if self.count < self.inner[SIZE_INDEX] as usize * 8 {
                None
            } else {
                Some(Ok(self.inner))
            }
        }
    }
}

pub struct Demodulator {
    window: VecDeque<i16>,
    state: DemodulateState,
    last_prod: i64,
    moving_average: i64,
}

impl Demodulator {
    const PREAMBLE_CHANNEL: usize = 0;
    const PREAMBLE_WAVE_LEN: usize = SECTION_LEN / (BASE_F + Self::PREAMBLE_CHANNEL);

    const PREAMBLE_LEN: usize = (SECTION_LEN + CYCLIC_PREFIX) * BARKER.len();
    const WINDOW_EXTRA_SIZE: usize = (SECTION_LEN + CYCLIC_PREFIX) * PRE_PREAMBLE.len();
    const WINDOW_CAPACITY: usize = Self::PREAMBLE_LEN + Self::WINDOW_EXTRA_SIZE;
    const HEADER_THRESHOLD_SCALE: i64 = 1 << 22;
    const MOVING_AVERAGE: i64 = 512;
    const ACTIVE_THRESHOLD: i64 = 256;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
    {
        iter_a.zip(iter_b).map(|(a, b)| a as i64 * b as i64).sum::<i64>()
    }

    fn preamble_product(&self) -> i64 {
        Self::dot_product(
            self.window.iter().skip(self.window.len() - Self::PREAMBLE_LEN).cloned(),
            bpsk_modulate(BARKER.iter().cloned()),
        )
    }

    fn section_product(&self, offset: usize, channel: usize) -> i64 {
        Self::dot_product(
            self.window.iter().skip(offset + CYCLIC_PREFIX).cloned(),
            carrier(channel, 0).take(SECTION_LEN),
        )
    }

    fn moving_average(last: i64, new: i64) -> i64 {
        (last * (Self::MOVING_AVERAGE - 1) + new) / Self::MOVING_AVERAGE
    }

    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(Self::WINDOW_CAPACITY),
            state: DemodulateState::WAITE,
            last_prod: 0,
            moving_average: 0,
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<DataPack> {
        if self.window.len() == Self::WINDOW_CAPACITY {
            self.window.pop_front();
        }

        self.window.push_back(item);

        self.moving_average = Self::moving_average(self.moving_average, (item as i64).abs());

        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;

        match self.state {
            DemodulateState::WAITE => {
                if self.window.len() >= Self::PREAMBLE_LEN &&
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

                self.state = if count >= Self::PREAMBLE_WAVE_LEN * 2 {
                    DemodulateState::WAITE
                } else {
                    count += 1;

                    if last_prod > prod && count >= Self::PREAMBLE_WAVE_LEN {
                        // print!("{} {} {} ", item, threshold, prod);

                        if last_prod < last {
                            if BARKER.len() - 1 <= BARKER.iter()
                                .enumerate().map(|(index, bit)| {
                                let shift = self.window.len() - Self::PREAMBLE_LEN - count;

                                let prod = self.section_product(
                                    shift + index * (SECTION_LEN + CYCLIC_PREFIX),
                                    Self::PREAMBLE_CHANNEL,
                                );

                                if *bit == (prod > 0) { 1 } else { 0 }
                            }).sum::<usize>() {
                                // println!("match {} {}", self.moving_average, last_prod);

                                self.last_prod = 0;

                                DemodulateState::RECEIVE(count, BitReceive::new())
                            } else {
                                // println!("preamble decode failed");

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

                self.state = if count == SECTION_LEN + CYCLIC_PREFIX {
                    for i in 0..CHANNEL {
                        let prod = self.section_product(self.window.len() - count, i);

                        if let Some(result) = buffer.push(prod > 0) {
                            self.state = DemodulateState::WAITE;

                            self.window.drain(..self.window.len() - SECTION_LEN);

                            self.state = DemodulateState::WAITE;

                            return match result {
                                Ok(data) => Some(data),
                                Err(_) => None,
                            };
                        }
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
