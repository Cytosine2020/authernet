use crate::DATA_PACK_SIZE;


pub type DataPack = [u8; DATA_PACK_SIZE / 8];

pub struct BitIter {
    inner: DataPack,
    count: usize,
}

impl BitIter {
    #[inline]
    pub fn new(inner: DataPack) -> Self {
        Self { inner, count: 0 }
    }
}

impl Iterator for BitIter {
    type Item = bool;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.count < DATA_PACK_SIZE {
            let ret = (self.inner[self.count / 8] >> self.count % 8) & 1 == 1;
            self.count += 1;
            Some(ret)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (DATA_PACK_SIZE, Some(DATA_PACK_SIZE))
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.count += n;
        if self.count > DATA_PACK_SIZE { self.count = DATA_PACK_SIZE; }
        self.next()
    }
}

#[derive(Copy, Clone)]
pub struct BitReceive {
    inner: DataPack,
    count: usize,
}

impl BitReceive {
    #[inline]
    pub fn new() -> Self { Self { inner: [0; DATA_PACK_SIZE / 8], count: 0 } }

    #[inline]
    pub fn push(&mut self, bit: bool) -> usize {
        self.inner[self.count / 8] |= (bit as u8) << (self.count % 8);
        self.count += 1;
        self.count
    }

    #[inline]
    pub fn into_array(self) -> DataPack { self.inner }
}
