use std::ops::Deref;


#[derive(Clone)]
struct ArcSlice<T> {
    inner: std::sync::Arc<[T]>,
}

impl<T> ArcSlice<T> {
    pub fn new(inner: std::sync::Arc<[T]>) -> Self { Self { inner } }
}

impl<T: Clone> ArcSlice<T> {
    fn iter(&self) -> impl Iterator<Item=T> {
        let clone = self.inner.clone();

        (0..self.inner.len()).cycle().map(move |i| clone[i].clone())
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
    rate: usize,
}

impl Wave {
    pub fn calculate(rate: f32, t: f32, amp: f32) -> f32 {
        (t * 2. * std::f32::consts::PI / rate).sin() * amp
    }

    pub fn new(rate: usize, amp: usize) -> Self {
        let wave = (0..1).map(|_| {
            (0..rate as usize).map(move |i| {
                Self::calculate(rate as f32, i as f32, amp as f32) as i16
            })
        }).flatten().collect();

        Self { wave: ArcSlice::new(wave), rate }
    }

    pub fn deep_clone(&self) -> Self {
        Self {
            wave: self.wave.deep_clone(),
            rate: self.rate
        }
    }

    pub fn iter(&self) -> impl Iterator<Item=i16> {
        self.wave.clone().iter()
    }
}
