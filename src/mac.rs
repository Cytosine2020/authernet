use lazy_static;
use crate::athernet::Athernet;


pub const DATA_PACK_MAX: usize = 80;
pub const CRC_SIZE: usize = 1;
pub const MAC_FRAME_MAX: usize = MacFrame::MAC_DATA_SIZE + DATA_PACK_MAX + CRC_SIZE;

pub type DataPack = [u8; DATA_PACK_MAX];
pub type MacFrameRaw = [u8; MAC_FRAME_MAX];


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

fn crc_calculate<I: Iterator<Item=u8>>(iter: I) -> u8 {
    iter.fold(0, |crc, byte| CRC_TABLE[(crc ^ byte) as usize])
}

#[derive(Copy, Clone)]
pub struct MacFrame {
    inner: MacFrameRaw,
}

impl MacFrame {
    pub const MAC_INDEX: usize = 0;
    pub const OP_INDEX: usize = Self::MAC_INDEX + 1;
    pub const MAC_DATA_SIZE: usize = Self::OP_INDEX + 1;

    pub const BROADCAST_MAC: u8 = 0b1111;

    pub const OP_DATA: u8 = 0b0000;
    pub const OP_PING_REQ: u8 = 0b0001;
    pub const OP_PING_REPLY: u8 = 0b0010;
    pub const OP_ACK: u8 = 0b1111;

    #[inline]
    pub fn new() -> Self { Self { inner: [0u8; MAC_FRAME_MAX] } }

    #[inline]
    fn set_src(&mut self, val: u8) -> &mut Self {
        self.inner[Self::MAC_INDEX] &= 0b11110000;
        self.inner[Self::MAC_INDEX] |= (val & 0b1111) << 0;
        self
    }

    #[inline]
    fn set_dest(&mut self, val: u8) -> &mut Self {
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
    fn set_pay_load(&mut self, data: &DataPack) -> &mut Self {
        let size = data[0] as usize + 1;
        self.inner[Self::MAC_DATA_SIZE..][..size].copy_from_slice(&data[..size]);
        self
    }

    #[inline]
    fn generate_crc(&mut self) -> &mut Self {
        let size = self.get_size();
        self.inner[size] = crc_calculate(self.inner[..size].iter().cloned());
        self
    }

    #[inline]
    pub fn wrap(src: u8, dest: u8, op: u8, tag: u8, data: &DataPack) -> Self {
        let mut result = Self::new();

        result
            .set_src(src)
            .set_dest(dest)
            .set_op(op)
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
    pub fn new_ping_request(src: u8, dest: u8, tag: u8) -> Self {
        let mut result = Self::new();

        result
            .set_src(src)
            .set_dest(dest)
            .set_op(Self::OP_PING_REQ)
            .set_tag(tag)
            .generate_crc();

        result
    }

    #[inline]
    pub fn new_ping_reply(src: u8, dest: u8, tag: u8) -> Self {
        let mut result = Self::new();

        result
            .set_src(src)
            .set_dest(dest)
            .set_op(Self::OP_PING_REPLY)
            .set_tag(tag)
            .generate_crc();

        result
    }

    #[inline]
    pub fn from_raw(inner: MacFrameRaw) -> Self { Self { inner } }

    #[inline]
    pub fn into_raw(self) -> MacFrameRaw { self.inner }

    #[inline]
    pub fn get_size(&self) -> usize {
        Self::MAC_DATA_SIZE + if self.is_data() {
            self.inner[Self::MAC_DATA_SIZE] as usize + 1
        } else {
            0
        }
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
    pub fn get_src(&self) -> u8 { (self.inner[Self::MAC_INDEX] >> 0) & 0b1111 }

    #[inline]
    pub fn get_dest(&self) -> u8 { (self.inner[Self::MAC_INDEX] >> 4) & 0b1111 }

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
    pub fn is_ping_request(&self) -> bool { self.get_op() == MacFrame::OP_PING_REQ }

    #[inline]
    pub fn check(&self, mac_addr: u8) -> bool {
        crc_calculate(self.inner[..self.get_size() + CRC_SIZE].iter().cloned()) == 0 &&
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
    pub fn new(mac_addr: u8, dest: u8, perf: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            athernet: Athernet::new(mac_addr, perf)?,
            send_tag: [0; 255],
            recv_tag: [0; 255],
            mac_addr,
            dest,
        })
    }

    pub fn send(&mut self, data: &DataPack) -> Result<(), Box<dyn std::error::Error>> {
        let send_tag = &mut self.send_tag[self.dest as usize];

        let tag = if self.dest == MacFrame::BROADCAST_MAC {
            0
        } else {
            let tag = *send_tag;
            *send_tag = send_tag.wrapping_add(1);
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
            let recv_tag = &mut self.recv_tag[src as usize];

            if (src, tag) == (self.dest, *recv_tag & 0b1111) {
                *recv_tag = recv_tag.wrapping_add(1);
                return Ok(mac_data.unwrap());
            }
        }
    }

    pub fn ping(&mut self)
                -> Result<Option<std::time::Duration>, Box<dyn std::error::Error>> {
        let send_tag = &mut self.send_tag[self.dest as usize];

        let time_out = std::time::Duration::from_secs(2);

        let start = std::time::SystemTime::now();

        self.athernet.send(MacFrame::new_ping_request(self.mac_addr, self.dest, *send_tag))?;

        loop {
            match self.athernet.ping_recv_timeout(time_out - start.elapsed()?) {
                Ok(pair) => {
                    if pair == (self.dest, *send_tag & 0b1111) {
                        *send_tag = send_tag.wrapping_add(1);
                        return Ok(Some(start.elapsed()?));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!(),
            };
        }
    }
}
