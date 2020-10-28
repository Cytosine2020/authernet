use std::collections::VecDeque;
use crate::{DATA_PACK_SIZE, DataPack, mac::{SIZE_INDEX, SIZE_SIZE}};
use lazy_static;


const SYMBOL_LEN: usize = 5;


lazy_static!(
    pub static ref CARRIER: [i16; SYMBOL_LEN] = {
        let mut carrier = [0i16; SYMBOL_LEN];

        const ZERO: f32 = SYMBOL_LEN as f32 / 2. - 0.5;

        for i in 0..SYMBOL_LEN {
            let t = (i as f32 - ZERO) * std::f32::consts::PI * 2. / SYMBOL_LEN as f32;

            let sinc = if t.abs() < 1e-6 {
                1.
            } else {
                t.sin() / t
            };

            carrier[i] = (sinc * std::i16::MAX as f32) as i16;
        }

        carrier
    };
);

fn carrier() -> impl Iterator<Item=i16> + 'static {
    CARRIER.iter().cloned()
}


const BARKER: [bool; 7] = [true, true, true, false, false, true, false];


pub struct ByteToBitIter<T> {
    iter: T,
    buffer: u8,
    index: u8,
}

impl<T> From<T> for ByteToBitIter<T> {
    fn from(iter: T) -> Self { Self { iter, buffer: 0, index: 8 } }
}

impl<T: Iterator<Item=u8>> Iterator for ByteToBitIter<T> {
    type Item = bool;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == 8 {
            if let Some(byte) = self.iter.next() {
                self.index = 0;
                self.buffer = byte;
            } else {
                return None;
            }
        };

        let index = self.index;
        self.index += 1;
        Some(((self.buffer >> index) & 1) == 1)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min, max) = self.iter.size_hint();
        let extra = 8 - self.index as usize;
        (min * 8 + extra, max.map(|value| value * 8 + extra))
    }
}


fn pulse_shaping<I: Iterator<Item=bool>>(iter: I) -> impl Iterator<Item=i16> {
    iter.map(move |bit| {
        carrier().map(move |item| if bit { item } else { -item })
    }).flatten()
}

pub struct Modulator {}

impl Modulator {
    pub fn new() -> Self { Self {} }

    pub fn iter(&self, buffer: DataPack) -> impl Iterator<Item=i16> {
        pulse_shaping(BARKER.iter().cloned()
            .chain(ByteToBitIter::from(
                (0..buffer[SIZE_INDEX] as usize).map(move |index| buffer[index])
            )))
    }
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
    pub fn push(&mut self, bit: bool) -> Option<Option<DataPack>> {
        self.inner[self.count / 8] |= (bit as u8) << (self.count % 8);
        self.count += 1;

        if self.count <= (SIZE_INDEX + SIZE_SIZE) * 8 {
            None
        } else {
            if self.inner[SIZE_INDEX] as usize > DATA_PACK_SIZE {
                Some(None)
            } else if self.count < self.inner[SIZE_INDEX] as usize * 8 {
                None
            } else {
                Some(Some(self.inner))
            }
        }
    }
}

enum DemodulateState {
    WAITE,
    RECEIVE(usize, BitReceive),
}

pub struct Demodulator {
    window: VecDeque<i16>,
    state: DemodulateState,
    last_prod: i64,
    moving_average: i64,
    eye_diagram: [usize; 256 * 2 * SYMBOL_LEN],
}

impl Demodulator {
    const PREAMBLE_LEN: usize = SYMBOL_LEN * BARKER.len();
    const HEADER_THRESHOLD_SCALE: i64 = 1 << 19;
    const MOVING_AVERAGE: i64 = 16;
    const ACTIVE_THRESHOLD: i64 = 128;

    fn dot_product<I, U>(iter_a: I, iter_b: U) -> i64
        where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
    {
        iter_a.zip(iter_b).map(|(a, b)| a as i64 * b as i64).sum::<i64>()
    }

    fn preamble_product(&self) -> i64 {
        Self::dot_product(
            self.window.iter().skip(self.window.len() - Self::PREAMBLE_LEN).cloned(),
            pulse_shaping(BARKER.iter().cloned()),
        )
    }

    fn section_product(&self, offset: usize) -> i64 {
        Self::dot_product(self.window.iter().skip(offset).cloned(), carrier())
    }

    fn moving_average(last: i64, new: i64) -> i64 {
        (last * (Self::MOVING_AVERAGE - 1) + new) / Self::MOVING_AVERAGE
    }

    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(Self::PREAMBLE_LEN),
            state: DemodulateState::WAITE,
            last_prod: 0,
            moving_average: 0,
            eye_diagram: [0; 256 * 2 * SYMBOL_LEN],
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<DataPack> {
        if self.window.len() == Self::PREAMBLE_LEN {
            self.window.pop_front();
        }

        self.window.push_back(item);

        self.moving_average = Self::moving_average(self.moving_average, (item as i64).abs());

        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;
        let mut prod = 0;

        match self.state {
            DemodulateState::WAITE => {
                if self.window.len() >= Self::PREAMBLE_LEN &&
                    self.moving_average > Self::ACTIVE_THRESHOLD {
                    prod = self.preamble_product();

                    if prod > threshold &&
                        self.last_prod > prod && BARKER.len() <= BARKER.iter()
                        .enumerate().map(|(index, bit)| {
                        let shift = self.window.len() - Self::PREAMBLE_LEN;

                        let prod = self.section_product(shift + index * SYMBOL_LEN);

                        if *bit == (prod > 0) { 1 } else { 0 }
                    }).sum::<usize>() {
                        self.state = DemodulateState::RECEIVE(0, BitReceive::new());
                        prod = 0;
                    }
                }
            }
            DemodulateState::RECEIVE(mut count, mut buffer) => {
                count += 1;

                self.state = if count == SYMBOL_LEN {
                    let prod = self.section_product(self.window.len() - SYMBOL_LEN);

                    for (index, item) in self.window.iter()
                        .skip(self.window.len() - SYMBOL_LEN).enumerate() {
                        let offset = (*item / 256) as usize + 128;

                        self.eye_diagram[index * 256 + offset] += 1;
                    }

                    if let Some(result) = buffer.push(prod > 0) {
                        self.state = DemodulateState::WAITE;
                        self.window.clear();
                        return result;
                    }

                    DemodulateState::RECEIVE(0, buffer)
                } else {
                    DemodulateState::RECEIVE(count, buffer)
                }
            }
        }

        self.last_prod = prod;

        // eprintln!("{}\t{}", threshold, self.last_prod);
        // eprintln!("{}", item);

        None
    }
}

// impl Drop for Demodulator {
//     fn drop(&mut self) {
//         for j in 0..256 {
//             for i in 0..SYMBOL_LEN {
//                 eprint!("{}\t", self.eye_diagram[i * 256 + j]);
//             }
//
//             for i in 0..SYMBOL_LEN {
//                 eprint!("{}\t", self.eye_diagram[i * 256 + j]);
//             }
//
//             for i in 0..SYMBOL_LEN {
//                 eprint!("{}\t", self.eye_diagram[i * 256 + j]);
//             }
//
//             eprintln!();
//         }
//     }
// }
