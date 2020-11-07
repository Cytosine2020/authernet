use std::collections::VecDeque;
use lazy_static;
use crate::mac::{MAC_FRAME_MAX, CRC_SIZE, MacFrame, MacFrameRaw};


const SYMBOL_LEN: usize = 5;
const BARKER: [bool; 7] = [true, true, true, false, false, true, false];


lazy_static!(
    static ref CARRIER: [i16; SYMBOL_LEN] = {
        let mut carrier = [0i16; SYMBOL_LEN];

        const ZERO: f32 = SYMBOL_LEN as f32 / 2. - 0.5;

        for i in 0..SYMBOL_LEN {
            let t = (i as f32 - ZERO) * std::f32::consts::PI * 2. / SYMBOL_LEN as f32;

            let sinc = if t.abs() < 1e-6 { 1. } else { t.sin() / t };

            carrier[i] = (sinc * std::i16::MAX as f32) as i16;
        }

        carrier
    };
);

fn carrier() -> impl Iterator<Item=i16> + 'static { CARRIER.iter().cloned() }


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

pub fn modulate(buffer: MacFrame) -> impl Iterator<Item=i16> {
    let size = buffer.get_size() + CRC_SIZE;
    let raw = buffer.into_raw();

    pulse_shaping(BARKER.iter().cloned().chain(ByteToBitIter::from(
        (0..size).map(move |index| raw[index])
    )))
}


#[derive(Copy, Clone)]
struct BitReceive {
    inner: MacFrameRaw,
    count: usize,
    mac_addr: u8,
}

impl BitReceive {
    #[inline]
    pub fn new(mac_addr: u8) -> Self { Self { inner: [0; MAC_FRAME_MAX], count: 0, mac_addr } }

    #[inline]
    pub fn push(&mut self, bit: bool) -> Option<Option<MacFrame>> {
        self.inner[self.count / 8] |= (bit as u8) << (self.count % 8);
        self.count += 1;

        if self.count <= (MacFrame::MAC_DATA_SIZE + CRC_SIZE) * 8 {
            None
        } else {
            let size = if self.inner[MacFrame::OP_INDEX] == MacFrame::OP_DATA {
                self.inner[MacFrame::MAC_DATA_SIZE] as usize + 1
            } else {
                0
            } + MacFrame::MAC_DATA_SIZE + CRC_SIZE;

            if size > MAC_FRAME_MAX { return Some(None); }

            if self.count < size * 8 {
                None
            } else {
                Some(Some(MacFrame::from_raw(self.inner)))
            }
        }
    }

    #[inline]
    pub fn is_self(&self) -> bool {
        self.count < 8 || self.inner[MacFrame::SRC_INDEX] == self.mac_addr
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
    jammed: usize,
    mac_addr: u8,
}

impl Demodulator {
    const PREAMBLE_LEN: usize = SYMBOL_LEN * BARKER.len();
    const HEADER_THRESHOLD_SCALE: i64 = 1 << 19;
    const MOVING_AVERAGE: i64 = 16;
    const ACTIVE_THRESHOLD: i64 = 512;
    const JAMMING_THRESHOLD: i64 = 4096;

    fn dot_product<I: Iterator<Item=i16>, U: Iterator<Item=i16>>(iter_a: I, iter_b: U) -> i64 {
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

    pub fn new(mac_addr: u8) -> Self {
        Self {
            window: VecDeque::with_capacity(Self::PREAMBLE_LEN),
            state: DemodulateState::WAITE,
            last_prod: 0,
            moving_average: 0,
            mac_addr,
            jammed: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        if self.jammed > 0 || self.moving_average > Self::JAMMING_THRESHOLD { return true; }

        if let DemodulateState::RECEIVE(_, receiver) = self.state {
            !receiver.is_self()
        } else {
            false
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<MacFrame> {
        self.jammed = self.jammed.saturating_sub(1);

        if self.window.len() == Self::PREAMBLE_LEN { self.window.pop_front(); }
        self.window.push_back(item);

        self.moving_average = Self::moving_average(self.moving_average, (item as i64).abs());
        let threshold = self.moving_average * Self::HEADER_THRESHOLD_SCALE;
        let mut prod = 0;

        match self.state {
            DemodulateState::WAITE => {
                if self.window.len() >= Self::PREAMBLE_LEN &&
                    self.moving_average > Self::ACTIVE_THRESHOLD {
                    prod = self.preamble_product();

                    if prod > threshold && self.last_prod > prod && BARKER.len() <= BARKER.iter()
                        .enumerate().map(|(index, bit)| {
                        let shift = self.window.len() - Self::PREAMBLE_LEN;

                        let prod = self.section_product(shift + index * SYMBOL_LEN);

                        if *bit == (prod > 0) { 1 } else { 0 }
                    }).sum::<usize>() {
                        self.state = DemodulateState::RECEIVE(0, BitReceive::new(self.mac_addr));
                        prod = 0;
                    }
                }
            }
            DemodulateState::RECEIVE(mut count, mut buffer) => {
                count += 1;

                self.state = if count == SYMBOL_LEN {
                    let prod = self.section_product(self.window.len() - SYMBOL_LEN);

                    if prod.abs() < (self.moving_average << 10) {
                        self.jammed = 64;
                        self.state = DemodulateState::WAITE;
                        self.window.clear();
                        return None;
                    } else if let Some(result) = buffer.push(prod > 0) {
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

        None
    }
}
