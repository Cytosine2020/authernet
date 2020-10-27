pub struct BitToByteIter<T> {
    iter: T,
}

impl<T> From<T> for BitToByteIter<T> {
    fn from(iter: T) -> Self { Self { iter } }
}

impl<T: Iterator<Item=bool>> Iterator for BitToByteIter<T> {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = 0;

        for i in 0..8 {
            if let Some(bit) = self.iter.next() {
                ret |= (bit as u8) << i;
            } else {
                if i == 0 { return None; }
            }
        }

        Some(ret)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min, max) = self.iter.size_hint();
        ((min + 7) / 8, max.map(|value| (value + 7) / 8))
    }
}


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
