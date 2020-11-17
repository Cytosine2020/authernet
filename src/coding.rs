pub const END: u8 = 0b01100;


/// 4b/5b decode
///
/// 0: None
/// 1: None
/// 2: None
/// 3: None
/// 4: None
/// 5: None
/// 6: None
/// 7: None
/// 8: None
/// 9: Some(Data(1))
/// 10: Some(Data(2))
/// 11: Some(Data(3))
/// 12: Some(End)
/// 13: Some(Data(5))
/// 14: Some(Data(6))
/// 15: Some(Data(7))
/// 16: None
/// 17: Some(Data(9))
/// 18: Some(Data(10))
/// 19: Some(Data(11))
/// 20: Some(Data(0))
/// 21: Some(Data(13))
/// 22: Some(Data(14))
/// 23: Some(Data(15))
/// 24: None
/// 25: Some(Data(4))
/// 26: Some(Data(8))
/// 27: Some(Data(12))
/// 28: Some(StartPingReply)
/// 29: Some(StartPingRequest)
/// 30: Some(StartAck)
/// 31: Some(StartData)

pub fn decode_5b(value: u8) -> Option<u8> {
    if (value & 0b11) == 0 || value > 0b11011 || value < 0b01001 {
        if value == 0b10100 {
            Some(0)
        } else {
            None
        }
    } else {
        let result = if value & 0b11100 == 0b11000 {
            (value & 0b11) << 2
        } else {
            value - 0b1000
        };

        Some(result)
    }
}


/// 4b/5b encode
///
/// 0: Some(20)
/// 1: Some(9)
/// 2: Some(10)
/// 3: Some(11)
/// 4: Some(25)
/// 5: Some(13)
/// 6: Some(14)
/// 7: Some(15)
/// 8: Some(26)
/// 9: Some(17)
/// 10: Some(18)
/// 11: Some(19)
/// 12: Some(27)
/// 13: Some(21)
/// 14: Some(22)
/// 15: Some(23)


pub fn encode_5b(value: u8) -> Option<u8> {
    if value > 0b1111 { return None; }

    let result = if value & 0b11 == 0 {
        if value == 0 {
            0b10100
        } else {
            (value >> 2) + 24
        }
    } else {
        value + 8
    };

    Some(result)
}

pub struct Encode4B5B<I> {
    iter: I,
    buffer: u16,
    index: u16,
}

impl<I> Encode4B5B<I> {
    pub fn new(iter: I) -> Self { Self { iter, buffer: 0, index: 10 } }
}

impl<I: Iterator<Item=u8>> Iterator for Encode4B5B<I> {
    type Item = bool;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == 10 {
            if let Some(next) = self.iter.next() {
                let first = encode_5b(next & 0b1111).unwrap();
                let next = encode_5b(next >> 4).unwrap();

                self.buffer = (first as u16) | ((next as u16) << 5);

                self.buffer;

                self.index = 0;
            } else {
                return None;
            };
        };

        let index = self.index;
        self.index += 1;
        Some((self.buffer >> index) & 1 == 1)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min, max) = self.iter.size_hint();
        let extra = 10 - self.index as usize;
        (min * 10 + extra, max.map(|value| value * 10 + extra))
    }
}

pub fn encode_4b_5b<I: Iterator<Item=u8>>(iter: I) -> Encode4B5B<I> {
    Encode4B5B::new(iter)
}

pub struct EncodeNRZI<I> {
    iter: I,
    last: bool,
}

impl<I> EncodeNRZI<I> {
    pub fn new(iter: I, last: bool) -> Self { Self { iter, last } }
}

impl<I: Iterator<Item=bool>> Iterator for EncodeNRZI<I> {
    type Item = bool;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(item) = self.iter.next() {
            self.last ^= item;
            Some(self.last)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) { self.iter.size_hint() }
}

pub type Return<T> = Option<Result<T, Box<dyn std::error::Error>>>;

pub trait Receiver {
    type Item;
    type Collection;

    fn push(&mut self, item: Self::Item) -> Return<Self::Collection>;

    fn peak(&self) -> (usize, &Self::Collection);
}

pub fn encode_nrzi<I: Iterator<Item=bool>>(iter: I, last: bool) -> EncodeNRZI<I> {
    EncodeNRZI::new(iter, last)
}

#[derive(Copy, Clone)]
pub struct DecodeNRZI<R> {
    receiver: R,
    last: bool,
}

impl<R> DecodeNRZI<R> {
    pub fn new(receiver: R, last: bool) -> Self { Self { receiver, last } }
}

impl<R: Receiver<Item=bool>> Receiver for DecodeNRZI<R> {
    type Item = bool;
    type Collection = R::Collection;

    fn push(&mut self, item: Self::Item) -> Return<Self::Collection> {
        let result = self.last ^ item;
        self.last = item;
        self.receiver.push(result)
    }

    fn peak(&self) -> (usize, &Self::Collection) {
        self.receiver.peak()
    }
}

pub fn decode_nrzi<R: Receiver<Item=bool>>(iter: R, last: bool) -> DecodeNRZI<R> {
    DecodeNRZI::new(iter, last)
}

#[derive(Copy, Clone)]
pub struct Decode4B5B<R> {
    receiver: R,
    buffer: u16,
    index: u16,
}

impl<R> Decode4B5B<R> {
    pub fn new(receiver: R) -> Self { Self { receiver, buffer: 0, index: 0 } }
}

impl<R: Receiver<Item=u8>> Decode4B5B<R> {
    const ERROR_MSG: &'static str = "unexpected symbol!";

    fn decode_byte(&mut self) -> Result<u8, Box<dyn std::error::Error>> {
        let first = decode_5b((self.buffer & 0b11111) as u8).ok_or(Self::ERROR_MSG)?;
        let second = decode_5b((self.buffer >> 5) as u8).ok_or(Self::ERROR_MSG)?;

        self.buffer = 0;
        self.index = 0;

        Ok(first | (second << 4))
    }
}

impl<R: Receiver<Item=u8>> Receiver for Decode4B5B<R> {
    type Item = bool;
    type Collection = R::Collection;

    fn push(&mut self, item: Self::Item) -> Return<Self::Collection> {
        self.buffer |= if item { 1 } else { 0 } << self.index;
        self.index += 1;

        if self.index == 10 {
            match self.decode_byte() {
                Ok(byte) => {
                    self.receiver.push(byte).map(|result| {
                        result.map_err(|err| err.into())
                    })
                }
                Err(error) => Some(Err(error)),
            }
        } else {
            None
        }
    }

    fn peak(&self) -> (usize, &Self::Collection) {
        self.receiver.peak()
    }
}
