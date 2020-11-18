use std::collections::VecDeque;
use crate::{
    mac::{MAC_FRAME_MAX, MacFrame, MacFrameRaw},
    coding::{Receiver, Return, DecodeNRZI, Decode4B5B, encode_4b_5b, encode_nrzi},
};


const SYMBOL_LEN: usize = 3;
const EXPANSION: usize = 5;
const PULSE_LEN: usize = SYMBOL_LEN * EXPANSION;


lazy_static!(
    pub static ref PULSE: [i16; PULSE_LEN] = {
        let mut carrier = [0i16; PULSE_LEN];

        const ZERO: f32 = PULSE_LEN as f32 / 2. - 0.5;

        for i in SYMBOL_LEN / 2..PULSE_LEN - SYMBOL_LEN / 2 {
            let t = (i as f32 - ZERO) * std::f32::consts::PI / SYMBOL_LEN as f32;

            let sinc = if t.abs() < 1e-6 { 1. } else { t.sin() / t };

            carrier[i] = (sinc * std::i16::MAX as f32 * 0.5) as i16;
        }

        carrier
    };
);


struct SlidingWindow<I: Iterator> {
    iter: I,
    buffer: VecDeque<Option<I::Item>>,
}

impl<I> SlidingWindow<I>
    where I: Iterator, I::Item: Clone,
{
    pub fn new(iter: I, size: usize) -> Self {
        Self { iter, buffer: std::iter::repeat(None).take(size).collect() }
    }
}

impl<I> Iterator for SlidingWindow<I>
    where I: Iterator, I::Item: Clone,
{
    type Item = Box<[Option<I::Item>]>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = if let Some(item) = self.iter.next() {
            Some(item)
        } else {
            if let None = self.buffer[0] { return None; }
            None
        };

        self.buffer.pop_front();
        self.buffer.push_back(item);

        Some(self.buffer.iter().cloned().collect())
    }
}

fn sliding_window<I>(iter: I, size: usize) -> SlidingWindow<I>
    where I: Iterator, I::Item: Clone,
{
    SlidingWindow::new(iter, size)
}


fn encode(buffer: MacFrame) -> impl Iterator<Item=bool> {
    let size = buffer.get_total_size();
    let raw = buffer.into_raw();

    encode_nrzi(encode_4b_5b((0..size).map(move |index| raw[index])), false)
}

pub fn modulate(frame: MacFrame) -> impl Iterator<Item=i16> {
    sliding_window([false, true, false].iter().cloned().chain(encode(frame)), EXPANSION)
        .map(move |window| {
            (0..SYMBOL_LEN).map(move |i|
                (0..EXPANSION).map(|j| PULSE[(EXPANSION - j - 1) * SYMBOL_LEN + i])
                    .zip(window.iter()).map(|(value, bit)| {
                    value * bit.map(|item| if item { 1 } else { -1 }).unwrap_or(0)
                }).sum::<i16>()
            )
        }).flatten()
}


#[derive(Copy, Clone)]
struct BitReceive {
    inner: MacFrameRaw,
    count: usize,
}

impl BitReceive {
    pub fn new() -> Self { Self { inner: [0; MAC_FRAME_MAX], count: 0 } }
}

impl Receiver for BitReceive {
    type Item = u8;
    type Collection = MacFrameRaw;

    fn push(&mut self, item: Self::Item) -> Return<Self::Collection> {
        self.inner[self.count] = item;
        self.count += 1;

        if self.count < MacFrame::MAC_DATA_SIZE + 1 {
            None
        } else {
            let size = if (self.inner[MacFrame::OP_INDEX] & 0b1111) == MacFrame::OP_DATA {
                self.inner[MacFrame::MAC_DATA_SIZE] as usize + 1 + 2
            } else {
                1
            } + MacFrame::MAC_DATA_SIZE;

            if size > MAC_FRAME_MAX { return Some(Err("mac frame size too big!".into())); }

            if self.count < size {
                None
            } else {
                Some(Ok(self.inner))
            }
        }
    }

    fn peak(&self) -> (usize, &Self::Collection) { (self.count, &self.inner) }
}

type Decoder = DecodeNRZI<Decode4B5B<BitReceive>>;

fn decoder(init: bool) -> Decoder { DecodeNRZI::new(Decode4B5B::new(BitReceive::new()), init) }

enum DemodulateState {
    Wait,
    Receive(usize, i16, Decoder),
}

pub struct Demodulator {
    window: VecDeque<i16>,
    state: DemodulateState,
    moving_average: i64,
    mac_addr: u8,
}

impl Demodulator {
    const PREAMBLE_LEN: usize = SYMBOL_LEN * 3;
    const MOVING_AVERAGE: i64 = 4;
    const ACTIVE_THRESHOLD: i64 = 1024;
    const JAMMING_THRESHOLD: i64 = 4096;

    fn moving_average(last: i64, new: i64) -> i64 {
        (last * (Self::MOVING_AVERAGE - 1) + new) / Self::MOVING_AVERAGE
    }

    pub fn new(mac_addr: u8) -> Self {
        Self {
            window: VecDeque::with_capacity(Self::PREAMBLE_LEN),
            state: DemodulateState::Wait,
            moving_average: 0,
            mac_addr,
        }
    }

    pub fn is_active(&self) -> bool {
        if self.moving_average > Self::JAMMING_THRESHOLD { return true; }

        if let DemodulateState::Receive(_, _, receiver) = self.state {
            let (count, data) = receiver.peak();
            count == 0 || data[0] & 0b1111 == self.mac_addr
        } else {
            false
        }
    }

    pub fn push_back(&mut self, item: i16) -> Option<MacFrame> {
        if self.window.len() == Self::PREAMBLE_LEN { self.window.pop_front(); }
        self.window.push_back(item);

        self.moving_average = Self::moving_average(self.moving_average, (item as i64).abs());

        match &mut self.state {
            DemodulateState::Wait => {
                if self.window.len() == Self::PREAMBLE_LEN &&
                    self.moving_average > Self::ACTIVE_THRESHOLD {
                    const INDEX: [usize; 3] = [1, 4, 7];

                    if INDEX.iter().cloned().all(|i| {
                        let item = self.window[i].abs();
                        item > self.window[i - 1].abs() && item > self.window[i + 1].abs()
                    }) {
                        let value = [
                            self.window[INDEX[0]],
                            self.window[INDEX[1]],
                            self.window[INDEX[2]],
                        ];

                        let avg = value.iter().map(|i| i.abs()).sum::<i16>() / 3;
                        let var = value.iter().map(|i| (i.abs() - avg).abs()).sum::<i16>() / 3;

                        if var * 4 < avg {
                            if value[0] < 0 && value[1] > 0 && value[2] < 0 {
                                self.state = DemodulateState::Receive(1, avg, decoder(false));
                            } else if value[0] > 0 && value[1] < 0 && value[2] > 0 {
                                self.state = DemodulateState::Receive(1, avg, decoder(true));
                            }
                        }
                    }
                }
            }
            DemodulateState::Receive(count, avg, buffer) => {
                if self.moving_average > Self::JAMMING_THRESHOLD {
                    // println!("jammed");
                    self.state = DemodulateState::Wait;
                    self.window.clear();
                    return None;
                }

                *count += 1;

                if *count % SYMBOL_LEN == 0 {
                    if (item.abs() - *avg).abs() * 2 > *avg {
                        // println!("difference too big");
                        self.state = DemodulateState::Wait;
                        self.window.clear();
                        return None;
                    }

                    if let Some(result) = buffer.push(item > 0) {
                        self.state = DemodulateState::Wait;
                        self.window.clear();
                        return result.map(|frame| MacFrame::from_raw(frame)).ok();
                    }
                };
            }
        }

        // eprintln!("{}", item);

        None
    }
}
