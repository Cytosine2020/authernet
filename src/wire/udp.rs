use crate::utils::slice_to_le_u16;
use std::fmt::Formatter;


pub const PROTOCOL: u8 = 17;

const HEADER_LEN: usize = 8;

type HeaderRaw = [u8; HEADER_LEN];

pub struct Header {
    inner: HeaderRaw,
}

impl Header {
    #[inline]
    pub fn new(src_port: u16, dest_port: u16, payload_length: usize) -> Self {
        let mut inner = [0; HEADER_LEN];

        inner[0..][..2].copy_from_slice(&src_port.to_le_bytes());
        inner[2..][..2].copy_from_slice(&dest_port.to_le_bytes());
        inner[4..][..2].copy_from_slice(&((payload_length + HEADER_LEN) as u16).to_le_bytes());

        Self { inner }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        let mut inner = [0; HEADER_LEN];
        inner.copy_from_slice(&data[..8]);
        Self { inner }
    }

    #[inline]
    pub fn get_src_port(&self) -> u16 { slice_to_le_u16(&self.inner[0..][..2]) }

    #[inline]
    pub fn get_dest_port(&self) -> u16 { slice_to_le_u16(&self.inner[2..][..2]) }

    #[inline]
    pub fn get_length(&self) -> usize { slice_to_le_u16(&self.inner[4..][..2]) as usize }

    #[inline]
    pub fn get_checksum(&self) -> u16 { slice_to_le_u16(&self.inner[6..][..2]) }

    #[inline]
    pub fn get_slice(&self) -> &[u8] { &self.inner }
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UDPHeader")
            .field("source port", &self.get_src_port())
            .field("destination port", &self.get_dest_port())
            .field("length", &self.get_length())
            .finish()
    }
}
