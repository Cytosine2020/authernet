use crate::utils::{slice_to_le_u16, slice_to_le_u32, ones_complete_checksum};
use std::fmt::Formatter;

pub type Address = [u8; 4];

pub const UNSPECIFIED: Address = [0x00; 4];
pub const BROADCAST: Address = [0xff; 4];


const HEADER_LEN: usize = 20;

type HeaderRaw = [u8; HEADER_LEN];

pub const VERSION: u8 = 4;

pub struct Header {
    inner: HeaderRaw,
}

impl Header {
    fn assemble_fragment(do_not_fragment: bool, more_fragment: bool, offset: u16) -> u16 {
        ((do_not_fragment as u16) << 1) | ((more_fragment as u16) << 2) | offset
    }

    #[inline]
    pub fn new(
        differentiated_services: u8,
        maximum_transmit_unit: usize, payload_length: usize,
        identification: u16,
        do_not_fragment: bool, offset: u16,
        time_to_live: u8, protocol: u8,
        src_ip: Address,
        dest_ip: Address,
    ) -> Self {
        let mut inner = [0; 20];

        let max_payload_length = (maximum_transmit_unit - HEADER_LEN) / 8 * 8;

        let (total_length, more_fragment) = if max_payload_length < payload_length {
            (max_payload_length + HEADER_LEN, true)
        } else {
            (payload_length + HEADER_LEN, false)
        };

        let fragment = Self::assemble_fragment(do_not_fragment, more_fragment, offset);

        inner[0] = VERSION & 0b1111 | ((HEADER_LEN / 4) as u8) << 4;
        inner[1] = differentiated_services;
        inner[2..][..2].copy_from_slice(&(total_length as u16).to_le_bytes());
        inner[4..][..2].copy_from_slice(&identification.to_le_bytes());
        inner[6..][..2].copy_from_slice(&fragment.to_le_bytes());
        inner[8] = time_to_live;
        inner[9] = protocol;
        inner[12..][..4].copy_from_slice(&src_ip);
        inner[16..][..4].copy_from_slice(&dest_ip);

        let checksum = ones_complete_checksum(&inner);
        inner[10..][..2].copy_from_slice(&checksum.to_le_bytes());

        Self { inner }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        let mut inner = [0; HEADER_LEN];
        inner.copy_from_slice(&data[..HEADER_LEN]);
        Self { inner }
    }

    #[inline]
    pub fn get_version(&self) -> u8 { self.inner[0] & 0b00001111 }

    #[inline]
    pub fn get_header_length(&self) -> usize { ((self.inner[0] >> 4) as usize) * 4 }

    #[inline]
    pub fn get_differential_service(&self) -> u8 { self.inner[1] }

    #[inline]
    pub fn get_total_length(&self) -> usize { slice_to_le_u16(&self.inner[2..][..2]) as usize }

    #[inline]
    pub fn get_payload_length(&self) -> usize {
        self.get_total_length() - self.get_header_length()
    }

    #[inline]
    pub fn get_identification(&self) -> u16 { slice_to_le_u16(&self.inner[4..][..2]) }

    #[inline]
    pub fn get_do_not_fragment(&self) -> bool {
        (slice_to_le_u16(&self.inner[6..][..2]) & 0b0000000000000010) > 0
    }

    #[inline]
    pub fn get_more_fragment(&self) -> bool {
        (slice_to_le_u16(&self.inner[6..][..2]) & 0b0000000000000100) > 0
    }

    #[inline]
    pub fn get_fragment_offset(&self) -> u16 {
        slice_to_le_u16(&self.inner[6..][..2]) & 0b1111111111111000
    }

    #[inline]
    pub fn get_time_to_live(&self) -> u8 { self.inner[8] }

    #[inline]
    pub fn get_protocol(&self) -> u8 { self.inner[9] }

    #[inline]
    pub fn get_src_ip(&self) -> Address {
        slice_to_le_u32(&self.inner[12..][..4]).to_le_bytes()
    }

    #[inline]
    pub fn get_dest_ip(&self) -> Address {
        slice_to_le_u32(&self.inner[16..][..4]).to_le_bytes()
    }

    #[inline]
    pub fn checksum(&self) -> bool {
        ones_complete_checksum(&self.inner[..self.get_header_length()]) == 0
    }

    #[inline]
    pub fn get_slice(&self) -> &[u8] { &self.inner }
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.checksum() {
            if self.get_header_length() == 20 {
                f.debug_struct("IPDatagram")
                    .field("version", &self.get_version())
                    .field("identification", &self.get_identification())
                    .field("do not fragment", &self.get_do_not_fragment())
                    .field("more fragment", &self.get_more_fragment())
                    .field("fragment offset", &self.get_fragment_offset())
                    .field("time to live", &self.get_time_to_live())
                    .field("protocol", &self.get_protocol())
                    .field("source ip", &self.get_src_ip())
                    .field("destination ip", &self.get_dest_ip())
                    .finish()
            } else {
                f.debug_struct("IPDatagram")
                    .field("header length", &self.get_header_length())
                    .finish()
            }
        } else {
            f.debug_struct("IPDatagram")
                .field("checksum", &self.checksum())
                .finish()
        }
    }
}
