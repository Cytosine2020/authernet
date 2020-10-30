use lazy_static;
use crate::acoustic::Athernet;


pub const DATA_PACK_MAX: usize = 256;
pub const CRC_SIZE: usize = 1;
pub const MAC_FRAME_MAX: usize = MacFrame::MAC_DATA_SIZE + DATA_PACK_MAX + CRC_SIZE;

pub type DataPack = [u8; DATA_PACK_MAX];
pub type MacFrameRaw = [u8; MAC_FRAME_MAX];


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

#[derive(Copy, Clone)]
pub struct MacFrame {
    inner: MacFrameRaw,
}

impl MacFrame {
    pub const DEST_INDEX: usize = 0;
    pub const SRC_INDEX: usize = Self::DEST_INDEX + 1;
    pub const OP_INDEX: usize = Self::SRC_INDEX + 1;
    pub const TAG_INDEX: usize = Self::OP_INDEX + 1;
    pub const MAC_DATA_SIZE: usize = Self::TAG_INDEX + 1;

    pub const BROADCAST_MAC: u8 = 0b11111111;

    pub const OP_DATA: u8 = 0b0000;
    pub const OP_PING_REQ: u8 = 0b0001;
    pub const OP_PING_REPLY: u8 = 0b0010;
    pub const OP_ACK: u8 = 0b1111;

    #[inline]
    pub fn wrap(src: u8, dest: u8, op: u8, tag: u8, data: &DataPack) -> Self {
        let mut inner = [0u8; MAC_FRAME_MAX];

        inner[Self::SRC_INDEX] = src;
        inner[Self::DEST_INDEX] = dest;
        inner[Self::OP_INDEX] = op;
        inner[Self::TAG_INDEX] = tag;
        inner[Self::MAC_DATA_SIZE..][..DATA_PACK_MAX].copy_from_slice(data);

        let mut ret = Self { inner };

        let size = ret.get_size();

        ret.inner[size] = crc_calculate(&ret.inner[..size]);

        ret
    }

    pub fn new_ack(src: u8, dest: u8, tag: u8) -> Self {
        let mut inner = [0u8; MAC_FRAME_MAX];

        inner[Self::SRC_INDEX] = src;
        inner[Self::DEST_INDEX] = dest;
        inner[Self::OP_INDEX] = Self::OP_ACK;
        inner[Self::TAG_INDEX] = tag;
        inner[Self::MAC_DATA_SIZE] = crc_calculate(&inner[..Self::MAC_DATA_SIZE]);

        Self { inner }
    }

    #[inline]
    pub fn from_raw(inner: MacFrameRaw) -> Self { Self { inner } }

    #[inline]
    pub fn into_raw(self) -> MacFrameRaw { self.inner }

    #[inline]
    pub fn get_size(&self) -> usize {
        Self::MAC_DATA_SIZE + if self.is_ack() {
            0
        } else {
            self.inner[Self::MAC_DATA_SIZE] as usize + 1
        }
    }

    #[inline]
    pub fn get_src(&self) -> u8 { self.inner[Self::SRC_INDEX] }

    #[inline]
    pub fn get_dest(&self) -> u8 { self.inner[Self::DEST_INDEX] }

    #[inline]
    pub fn get_op(&self) -> u8 { self.inner[Self::OP_INDEX] }

    #[inline]
    pub fn get_tag(&self) -> u8 { self.inner[Self::TAG_INDEX] }

    #[inline]
    pub fn to_broadcast(&self) -> bool {
        self.get_dest() == MacFrame::BROADCAST_MAC
    }

    #[inline]
    pub fn is_ack(&self) -> bool {
        self.get_op() == MacFrame::OP_ACK
    }

    #[inline]
    pub fn check(&self, mac_addr: u8) -> bool {
        crc_calculate(&self.inner[..self.get_size() + CRC_SIZE]) == 0 &&
            (self.get_dest() == mac_addr || self.get_dest() == MacFrame::BROADCAST_MAC)
    }

    #[inline]
    pub fn unwrap(&self) -> DataPack {
        let mut data_pack = [0u8; DATA_PACK_MAX];
        let size = self.get_size() - Self::MAC_DATA_SIZE;
        data_pack[..size].copy_from_slice(&self.inner[Self::MAC_DATA_SIZE..][..size]);
        data_pack
    }
}

pub struct MacLayer {
    athernet: Athernet,
    send_tag: [u8; 255],
    recv_tag: [u8; 255],
    mac_addr: u8,
    dest: u8,
}

impl MacLayer {
    pub fn new(mac_addr: u8, dest: u8) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            athernet: Athernet::new(mac_addr)?,
            send_tag: [0; 255],
            recv_tag: [0; 255],
            mac_addr,
            dest,
        })
    }

    pub fn send(&mut self, data: &DataPack) -> Result<(), Box<dyn std::error::Error>> {
        let dest = self.dest as usize;

        let tag = if self.dest == MacFrame::BROADCAST_MAC {
            0
        } else {
            let tag = self.send_tag[dest];
            self.send_tag[dest] = self.send_tag[dest].wrapping_add(1);
            tag
        };

        Ok(self.athernet.send(
            MacFrame::wrap(self.mac_addr, self.dest, MacFrame::OP_DATA, tag, data)
        )?)
    }

    pub fn recv(&mut self) -> Result<DataPack, Box<dyn std::error::Error>> {
        loop {
            let mac_data = self.athernet.recv()?;
            let src = mac_data.get_src();
            let tag = mac_data.get_tag();

            if src == self.dest && self.recv_tag[src as usize] == tag {
                self.recv_tag[src as usize] = self.recv_tag[src as usize].wrapping_add(1);

                return Ok(mac_data.unwrap())
            }
        }
    }
}
