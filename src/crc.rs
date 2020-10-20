use crate::{DATA_PACK_SIZE, DataPack};
use lazy_static;


pub const PAYLOAD_SIZE: usize = DATA_PACK_SIZE - 3;


lazy_static!(
    static ref CRC_TABLE: [u8; 256] = {
        let mut table = [0; 256];

        for i in 0..0xFFu8 {
            let mut crc = i;

            for _ in 0..=8 {
                if (crc & 0x80) > 0 {
                    crc = (crc << 1) ^ 0x31;
                } else {
                    crc <<= 1;
                }
            }
            table[i as usize] = crc;
        }

        table
    };
);

fn crc_generate(data: &[u8]) -> u8 {
    let mut crc = 0;

    for byte in data.iter() {
        crc = crc ^ *byte;
        crc = CRC_TABLE[crc as usize];
    }

    crc
}

pub struct FileRead<T> {
    iter: T,
    count: u8,
}

impl<T> FileRead<T> {
    pub fn new(iter: T) -> Self { Self { iter, count: 0 } }
}

impl<T: Iterator<Item=u8>> Iterator for FileRead<T> {
    type Item = DataPack;

    fn next(&mut self) -> Option<Self::Item> {
        let mut ret = [0; DATA_PACK_SIZE];

        ret[1] = self.count;

        self.count += 1;

        let mut size = DATA_PACK_SIZE;

        for i in 0..PAYLOAD_SIZE {
            if let Some(byte) = self.iter.next() {
                ret[i + 2] = byte;
            } else {
                if i == 0 { return None; }
                size = i + 3;
                break;
            }
        }

        ret[0] = size as u8;
        ret[size - 1] = crc_generate(&ret[..size - 1]);

        Some(ret)
    }
}

pub fn crc_unwrap(data: & DataPack) -> Option<&[u8]> {
    let size = data[0] as usize;

    if size <= DATA_PACK_SIZE && crc_generate(&data[..size - 1]) == data[size - 1] {
        Some(&data[..size])
    } else {
        None
    }
}
