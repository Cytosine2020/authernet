use std::ops::Deref;


#[derive(Clone)]
struct ArcSlice<T> {
    inner: std::sync::Arc<[T]>,
}

impl<T> ArcSlice<T> {
    pub fn new(inner: std::sync::Arc<[T]>) -> Self { Self { inner } }
}

impl<T: Clone> ArcSlice<T> {
    fn iter(&self, shift: usize) -> impl Iterator<Item=T> {
        let clone = self.inner.clone();

        (0..self.inner.len()).cycle().skip(shift).map(move |i| clone[i].clone())
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
    wave: Vec<ArcSlice<i16>>,
    rate: usize,
}

impl Wave {
    pub fn calculate(rate: f32, t: f32, amp: f32) -> f32 {
        (t * 2. * std::f32::consts::PI / rate).sin() * amp
    }

    pub fn new(rate: usize, amp: usize) -> Self {
        let wave = (0..1).map(|_| {
            let wave = (0..rate as usize).map(move |i| {
                Self::calculate(rate as f32, i as f32, amp as f32) as i16
            }).collect();

            ArcSlice::new(wave)
        }).collect::<Vec<_>>();

        Self { wave, rate }
    }

    pub fn deep_clone(&self) -> Self {
        Self {
            wave: self.wave.iter().map(|item| item.deep_clone()).collect(),
            rate: self.rate
        }
    }

    pub fn get_rate(&self) -> usize { self.rate }

    pub fn iter(&self, t: usize) -> impl Iterator<Item=i16> {
        self.wave[0].clone().iter(t % self.rate as usize)
    }
}
