use crate::physical::{PHY_PAYLOAD_MAX, PhyPayload};


// pub const CRC_SIZE: usize = 2;
// pub const MAC_PAYLOAD_MAX: usize = PHY_PAYLOAD_MAX - MacFrame::MAC_DATA_SIZE - CRC_SIZE;
//
// pub type MacPayload = [u8; MAC_PAYLOAD_MAX];

pub type MacAddress = u8;


lazy_static!(
    static ref CRC_TABLE: [u8; 256] = {
        let mut table = [0; 256];

        for i in 0..256 {
            table[i] = (0..=8).fold(i as u8, |crc, _| {
                (crc << 1) ^ if (crc & 0x80) > 0 { 0x31 } else { 0 }
            });
        }

        table
    };
);

pub fn crc8_checksum<I: Iterator<Item=u8>>(iter: I) -> u8 {
    iter.fold(0, |crc, byte| CRC_TABLE[(crc ^ byte) as usize])
}

pub fn crc16_checksum<I: Iterator<Item=u8>>(iter: I) -> u16 {
    let buffer = iter.collect::<Vec<_>>();

    crc16::State::<crc16::ARC>::calculate(buffer.as_slice())
}


#[derive(Copy, Clone)]
pub struct MacFrame {
    inner: PhyPayload,
}

impl MacFrame {
    pub const MAC_INDEX: usize = 0;
    pub const OP_INDEX: usize = Self::MAC_INDEX + 1;
    pub const MAC_DATA_SIZE: usize = Self::OP_INDEX + 1;

    pub const BROADCAST_MAC: u8 = 0b1111;

    pub const OP_DATA: u8 = 0b0000;
    pub const OP_ACK: u8 = 0b1111;

    #[inline]
    pub fn new() -> Self { Self { inner: [0u8; PHY_PAYLOAD_MAX] } }

    #[inline]
    fn set_src(&mut self, val: MacAddress) -> &mut Self {
        self.inner[Self::MAC_INDEX] &= 0b11110000;
        self.inner[Self::MAC_INDEX] |= (val & 0b1111) << 0;
        self
    }

    #[inline]
    fn set_dest(&mut self, val: MacAddress) -> &mut Self {
        self.inner[Self::MAC_INDEX] &= 0b00001111;
        self.inner[Self::MAC_INDEX] |= (val & 0b1111) << 4;
        self
    }

    #[inline]
    fn set_op(&mut self, val: u8) -> &mut Self {
        self.inner[Self::OP_INDEX] &= 0b11110000;
        self.inner[Self::OP_INDEX] |= (val & 0b1111) << 0;
        self
    }

    #[inline]
    fn set_tag(&mut self, val: u8) -> &mut Self {
        self.inner[Self::OP_INDEX] &= 0b00001111;
        self.inner[Self::OP_INDEX] |= (val & 0b1111) << 4;
        self
    }

    #[inline]
    fn set_pay_load(&mut self, data: &[u8]) -> &mut Self {
        let size = data.len();
        self.inner[Self::MAC_DATA_SIZE] = size as u8;
        self.inner[Self::MAC_DATA_SIZE + 1..][..size].copy_from_slice(&data[..size]);
        self
    }

    #[inline]
    fn generate_crc(&mut self) -> &mut Self {
        let size = self.get_size();

        if self.is_data() {
            let crc = crc16_checksum(self.inner[..size].iter().cloned());
            self.inner[size + 0] = ((crc >> 0) & 0b11111111) as u8;
            self.inner[size + 1] = ((crc >> 8) & 0b11111111) as u8;
            self
        } else {
            let size = self.get_size();
            self.inner[size] = crc8_checksum(self.inner[..size].iter().cloned());
            self
        }
    }

    #[inline]
    pub fn new_data(src: u8, dest: u8, tag: u8, data: &[u8]) -> Self {
        let mut result = Self::new();

        result
            .set_src(src)
            .set_dest(dest)
            .set_op(Self::OP_DATA)
            .set_tag(tag)
            .set_pay_load(data)
            .generate_crc();

        result
    }

    #[inline]
    pub fn new_ack(src: u8, dest: u8, tag: u8) -> Self {
        let mut result = Self::new();

        result
            .set_src(src)
            .set_dest(dest)
            .set_op(Self::OP_ACK)
            .set_tag(tag)
            .generate_crc();

        result
    }

    #[inline]
    pub fn from_raw(inner: PhyPayload) -> Self { Self { inner } }

    #[inline]
    pub fn into_raw(self) -> PhyPayload { self.inner }

    #[inline]
    pub fn get_size(&self) -> usize {
        Self::MAC_DATA_SIZE + if self.is_data() {
            self.inner[Self::MAC_DATA_SIZE] as usize + 1
        } else {
            0
        }
    }

    #[inline]
    pub fn get_crc_size(&self) -> usize {
        if self.is_data() {
            2
        } else {
            1
        }
    }

    #[inline]
    pub fn get_total_size(&self) -> usize {
        self.get_size() + self.get_crc_size()
    }

    #[inline]
    pub fn get_payload_size(&self) -> usize {
        if self.is_data() {
            self.inner[Self::MAC_DATA_SIZE] as usize
        } else {
            0
        }
    }

    #[inline]
    pub fn get_src(&self) -> MacAddress { (self.inner[Self::MAC_INDEX] >> 0) & 0b1111 }

    #[inline]
    pub fn get_dest(&self) -> MacAddress { (self.inner[Self::MAC_INDEX] >> 4) & 0b1111 }

    #[inline]
    pub fn get_op(&self) -> u8 { (self.inner[Self::OP_INDEX] >> 0) & 0b1111 }

    #[inline]
    pub fn get_tag(&self) -> u8 { (self.inner[Self::OP_INDEX] >> 4) & 0b1111 }

    #[inline]
    pub fn to_broadcast(&self) -> bool { self.get_dest() == MacFrame::BROADCAST_MAC }

    #[inline]
    pub fn is_ack(&self) -> bool { self.get_op() == MacFrame::OP_ACK }

    #[inline]
    pub fn is_data(&self) -> bool { self.get_op() == MacFrame::OP_DATA }

    #[inline]
    pub fn check(&self, mac_addr: u8) -> bool {
        let crc_flag = if self.is_data() {
            crc16_checksum(self.inner[..self.get_total_size()].iter().cloned()) == 0
        } else {
            crc8_checksum(self.inner[..self.get_total_size()].iter().cloned()) == 0
        };

        crc_flag && (self.get_dest() == mac_addr || self.get_dest() == MacFrame::BROADCAST_MAC)
    }

    #[inline]
    pub fn unwrap(&self) -> Box<[u8]> {
        let size = self.get_payload_size();
        self.inner[Self::MAC_DATA_SIZE + 1..][..size].iter().cloned().collect()
    }
}
