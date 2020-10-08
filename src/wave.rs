use std::ops::Deref;
use crate::CHANNEL;


#[derive(Clone)]
struct ArcSlice<T> {
    inner: std::sync::Arc<[T]>,
}

impl<T> ArcSlice<T> {
    pub fn new(inner: std::sync::Arc<[T]>) -> Self { Self { inner } }
}

impl<T: Clone> ArcSlice<T> {
    fn iter(&self, start: usize, end: usize, shift: usize) -> impl Iterator<Item=T> {
        let clone = self.inner.clone();

        (start..end).cycle().skip(shift).map(move |i| clone[i].clone())
    }

    pub fn deep_clone(&self) -> Self {
        Self { inner: (*self.inner).into() }
    }
}

impl<T> Deref for ArcSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target { &*self.inner }
}

#[derive(Clone)]
pub struct Wave {
    wave: ArcSlice<i16>,
    section: usize,
}

impl Wave {
    pub fn calculate(rate: f32, t: f32, amp: f32) -> f32 {
        (t * 2. * std::f32::consts::PI / rate).sin() * amp
    }

    pub fn new(section: usize, amp: usize, base_f: usize, channel: usize) -> Self {
        let wave = (base_f..(base_f + channel)).map(|f| {
            (0..section as usize).map(move |i| {
                Self::calculate(section as f32 / f as f32, i as f32, amp as f32) as i16
            })
        }).flatten().collect();

        Self { wave: ArcSlice::new(wave), section }
    }

    pub fn deep_clone(&self) -> Self {
        Self { wave: self.wave.deep_clone(), section: self.section }
    }

    pub fn iter(&self, channel: usize, shift: usize) -> impl Iterator<Item=i16> {
        self.wave.clone().iter(channel * self.section, (channel + 1) * self.section, shift)
    }
}

pub struct Synthesizer<T> {
    iters: Box<[T]>,
}

impl<T> Synthesizer<T> {
    pub fn new<B: Iterator<Item=T>>(iters: B) -> Self {
        Self { iters: iters.collect() }
    }
}

impl<T: Iterator<Item=i16>> Iterator for Synthesizer<T> {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let mut sum = 0;

        for iter in self.iters.iter_mut() {
            match iter.next() {
                Some(item) => sum += item as isize,
                None => return None,
            };
        }

        Some((sum / CHANNEL as isize) as i16)
    }
}
