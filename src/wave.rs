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

    pub fn iter(&self, t: usize) -> WaveIter { WaveIter::new(*self, t) }
}

pub struct WaveIter {
    wave: Wave,
    t: usize,
}

impl WaveIter {
    pub fn new(wave: Wave, t: usize) -> Self { Self { wave, t: t % wave.get_rate() } }
}

impl Iterator for WaveIter {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.wave.calculate(self.t);
        self.t += 1;
        if self.t == self.wave.get_rate() { self.t = 0; }
        Some(ret)
    }
}
