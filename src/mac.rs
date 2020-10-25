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

    pub const BROADCAST_MAC: u8 = 0b111111;

    pub const DATA: u8 = 0b0000;
    pub const ACK: u8 = 0b1111;

    #[inline]
    pub fn copy_from_slice(data_: &DataPack) -> Self {
        let mut data = [0u8; 2];
        data.copy_from_slice(&data_[MAC_INDEX..MAC_INDEX + MAC_SIZE]);
        Self { inner: u16::from_le_bytes(data) }
    }

    #[inline]
    pub fn copy_to_slice(&self, data_pack: &mut DataPack) {
        data_pack[MAC_INDEX..MAC_INDEX + MAC_SIZE].copy_from_slice(&self.inner.to_le_bytes());
    }

    #[inline]
    pub fn new(src: u8, dest: u8, op: u8) -> Self {
        let inner = (((op & Self::OP_MASK) as u16) << Self::OP_OFFSET) |
            (((dest & Self::MAC_MASK) as u16) << Self::DEST_OFFSET) |
            (((src & Self::MAC_MASK) as u16) << Self::SRC_OFFSET);
        Self { inner }
    }

    #[inline]
    pub fn get_op(&self) -> u8 { (self.inner >> Self::OP_OFFSET) as u8 & Self::OP_MASK }

    #[inline]
    pub fn get_dest(&self) -> u8 { (self.inner >> Self::DEST_OFFSET) as u8 & Self::MAC_MASK }

    #[inline]
    pub fn get_src(&self) -> u8 { (self.inner >> Self::SRC_OFFSET) as u8 & Self::MAC_MASK }

    #[inline]
    pub fn get_mac(&self) -> (u8, u8) { (self.get_src(), self.get_dest()) }
}

#[derive(Clone)]
pub struct MacLayer {
    mac_addr: u8,
}

impl MacLayer {
    #[inline]
    pub fn new(mac_addr: u8) -> Self { Self { mac_addr } }

    pub fn wrap(&self, dest: u8, op: u8, data: &[u8]) -> DataPack {
        let mut result: DataPack = [0; DATA_PACK_SIZE];

        MacData::new(self.mac_addr, dest, op).copy_to_slice(&mut result);

        let size = BODY_INDEX + data.len();

        result[SIZE_INDEX] = size as u8;
        result[BODY_INDEX..size].copy_from_slice(data);
        result[size] = crc_calculate(&data[..size]);

        result
    }

    pub fn create_ack(&self, dest: u8) -> DataPack {
        self.wrap(dest, MacData::ACK, &[])
    }

    pub fn unwrap<'a>(&self, data: &'a DataPack) -> Option<(MacData, &'a [u8])> {
        let size = data[SIZE_INDEX] as usize;
        let mac_data = MacData::copy_from_slice(&data);

        if size > 1 && crc_calculate(&data[..size]) == 0 &&
            mac_data.get_src() != self.mac_addr &&
            (mac_data.get_dest() == self.mac_addr ||
                mac_data.get_dest() == MacData::BROADCAST_MAC) {
            Some((mac_data, &data[BODY_INDEX..size - CRC_SIZE]))
        } else {
            None
        }
    }
}
