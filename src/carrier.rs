use lazy_static;

pub const SECTION_LEN: usize = 6;
pub const BASE_F: usize = 1;
pub const CHANNEL: usize = 1;


lazy_static!(
    static ref CARRIER: [i16; CHANNEL * SECTION_LEN] = {
        let mut wave = [0i16; CHANNEL * SECTION_LEN];

        for i in 0..CHANNEL * SECTION_LEN {
            let rate = SECTION_LEN as f32 / (i / SECTION_LEN + BASE_F) as f32;
            let t = (i % SECTION_LEN) as f32;
            wave[i] = ((t * 2. * std::f32::consts::PI / rate).sin() * std::i16::MAX as f32) as i16;
        }

        wave
    };
);

pub fn carrier(channel: usize) -> impl Iterator<Item=i16> + 'static {
    CARRIER[channel * SECTION_LEN..(channel + 1) * SECTION_LEN].iter().cloned()
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
