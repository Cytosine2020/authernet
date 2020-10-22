use crate::{DATA_PACK_SIZE, DataPack};
use lazy_static;


pub const SIZE_INDEX: usize = 0;
pub const SIZE_SIZE: usize = 1;
pub const MAC_INDEX: usize = SIZE_INDEX + SIZE_SIZE;
pub const MAC_SIZE: usize = 2;
pub const BODY_INDEX: usize = MAC_INDEX + MAC_SIZE;
pub const BODY_MAX_SIZE: usize = DATA_PACK_SIZE - BODY_INDEX - CRC_SIZE;
pub const CRC_SIZE: usize = 1;


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

fn crc_calculate(data: &[u8]) -> u8 {
    let mut crc = 0;

    for byte in data.iter() {
        crc = crc ^ *byte;
        crc = CRC_TABLE[crc as usize];
    }

    crc
}

pub fn crc_generate(data: &mut DataPack) {
    let size = data[SIZE_INDEX] as usize;

    data[size - CRC_SIZE] = crc_calculate(&data[..size - CRC_SIZE]);
}

pub fn crc_unwrap(data: &DataPack) -> Option<&[u8]> {
    let size = data[SIZE_INDEX] as usize;

    if size > 1 && crc_calculate(&data[..size - CRC_SIZE]) == data[size - CRC_SIZE] {
        Some(&data[BODY_INDEX..size - CRC_SIZE])
    } else {
        None
    }
}

pub struct MacData {
    inner: u16,
}

impl MacData {
    const MAC_SIZE: u8 = 6;
    const OP_SIZE: u8 = 4;
    const MAC_MASK: u8 = (1 << Self::MAC_SIZE) - 1;
    const OP_MASK: u8 = (1 << Self::OP_SIZE) - 1;
    const OP_OFFSET: u16 = 0;
    const DEST_OFFSET: u16 = Self::OP_OFFSET + Self::OP_SIZE as u16;
    const SRC_OFFSET: u16 = Self::DEST_OFFSET + Self::MAC_SIZE as u16;

    pub fn from_slice(data: [u8; 2]) -> Self { Self { inner: u16::from_le_bytes(data) } }

    pub fn get_op(&self) -> u8 { (self.inner >> Self::OP_OFFSET) as u8 & Self::OP_MASK }

    pub fn get_dest(&self) -> u8 { (self.inner >> Self::DEST_OFFSET) as u8 & Self::MAC_MASK }

    pub fn get_src(&self) -> u8 { (self.inner >> Self::SRC_OFFSET) as u8 & Self::MAC_MASK }
}
