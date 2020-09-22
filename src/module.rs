use std::{
    borrow::Borrow,
    collections::VecDeque,
};
use crate::{
    DATA_PACK_SIZE, BARKER, SECTION_LEN, WAVE_LENGTH,
    bit_set::{DataPack, BitReceive, BitIter},
};


#[derive(Copy, Clone)]
pub struct Wave {
    rate: usize,
    amp: usize,
}

impl Wave {
    pub fn calculate(&self, t: usize) -> i16 {
        ((t as f32 * 2. * std::f32::consts::PI / self.rate as f32).sin() * self.amp as f32) as i16
    }

    pub fn new(rate: usize, amp: usize) -> Self { Self { rate, amp } }

    pub fn get_rate(&self) -> usize { self.rate }

    pub fn iter(&self, t: usize) -> impl Iterator<Item=i16> {
        let mut wave = (0..self.rate).into_iter()
            .map(|i| self.calculate(i))
            .collect::<VecDeque<_>>();

        wave.rotate_left(t % self.rate);

        wave.into_iter().cycle()
    }
}

fn bpsk_modulate<I>(iter: I, carrier: Wave, len: usize) -> impl Iterator<Item=i16>
    where I: Iterator, I::Item: Borrow<bool>,
{
    iter.map(move |bit| {
        carrier.iter(*bit.borrow() as usize * carrier.get_rate() / 2).take(len)
    }).flatten()
}

pub struct Modulator {
    carrier: Wave,
    len: usize,
}

impl Modulator {
    pub fn new(carrier: Wave, len: usize) -> Self { Self { carrier, len } }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        bpsk_modulate(
            std::iter::empty()
                // .chain([false, false, false, false, false].iter().cloned())
                .chain(BARKER.iter().cloned())
                .chain(BitIter::new(buffer)),
            self.carrier, self.len)
    }
}

enum DemodulateState {
    WAITE,
    MATCH(usize, i64),
    RECEIVE(Vec<i16>, BitReceive),
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
    const HEADER_THRESHOLD_SCALE: i64 = 1 << 23;
    const MOVING_AVERAGE: i64 = 512;
    const ACTIVE_THRESHOLD: i64 = 1024;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator, I::Item: Borrow<i16>,
              U: Iterator, U::Item: Borrow<i16>,
    {
        iter_a.zip(iter_b)
            .map(|(a, b)| *a.borrow() as i64 * *b.borrow() as i64)
            .sum::<i64>()
    }

    fn moving_average(last: i64, new: i64, constant: i64) -> i64 {
        (last * (constant - 1) + new) / constant
    }

    pub fn new<I>(preamble_: I, carrier: Wave, len: usize) -> Self
        where I: Iterator, I::Item: Borrow<bool>,
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

    fn receive(&mut self, item: i16) -> Option<DataPack> {
        if let DemodulateState::RECEIVE(
            ref mut wave_buffer,
            ref mut data_buffer,
        ) = self.state {
            wave_buffer.push(item);

            if wave_buffer.len() == SECTION_LEN {
                let prod = Self::dot_product(wave_buffer.iter(), self.carrier.iter(0));

                wave_buffer.clear();

                if data_buffer.push(prod < 0) == DATA_PACK_SIZE {
                    let result = data_buffer.into_array();

                    self.state = DemodulateState::WAITE;

                    return Some(result);
                }
            }
        }

        None
    }

    pub fn push_back(&mut self, item: i16) -> Option<DataPack> {
        if self.window.len() == self.preamble.len() {
            self.window.pop_front();
        }

        self.window.push_back(item);

        let ret = self.receive(item);

        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;

        if self.window.len() == self.preamble.len() {
            let prod = Self::dot_product(self.window.iter(), self.preamble.iter());

            self.moving_average = Self::moving_average(
                self.moving_average, (item as i64).abs(), Self::MOVING_AVERAGE,
            );

            let flag = self.moving_average > Self::ACTIVE_THRESHOLD && self.last_prod > prod;

            match self.state {
                DemodulateState::WAITE => {
                    if flag && prod > threshold {
                        // print!("{} {} {} ", item, threshold, prod);

                        self.state = DemodulateState::MATCH(1, self.last_prod);
                    }
                }
                DemodulateState::MATCH(ref mut count, ref mut last) => {
                    if *count >= WAVE_LENGTH * 2 {
                        self.state = DemodulateState::WAITE;
                    } else {
                        *count += 1;

                        if flag && *count >= WAVE_LENGTH {
                            // print!("{} {} {} ", item, threshold, prod);

                            if self.last_prod < *last {
                                let count_copy = *count;

                                self.state = DemodulateState::RECEIVE(
                                    Vec::with_capacity(SECTION_LEN),
                                    BitReceive::new(),
                                );

                                for i in self.window.len() - count_copy..self.window.len() {
                                    self.receive(self.window[i]);
                                }

                                // println!("match");
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

            eprintln!("{}\t{}", threshold, prod);
        } else {
            // eprintln!("{}\t{}", threshold, 0);
        }

        ret
    }
}
