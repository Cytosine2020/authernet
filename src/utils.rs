macro_rules! make_slice_to_le_integer {
    ($name:ident, $int_type:ty) => {
        #[inline]
        pub fn $name(value: &[u8]) -> $int_type {
            let mut buffer = [0; std::mem::size_of::<$int_type>()];
            buffer.copy_from_slice(value);
            <$int_type>::from_le_bytes(buffer)
        }
    }
}

make_slice_to_le_integer!(slice_to_le_u16, u16);
make_slice_to_le_integer!(slice_to_le_u32, u32);
make_slice_to_le_integer!(slice_to_le_u64, u64);


pub fn ones_complete_checksum(data: &[u8]) -> u16 {
    let mut checksum: u16 = 0;

    for i in 0..data.len() / 2 {
        let value = slice_to_le_u16(&data[i * 2..][..2]);
        let (sum, overflow) = checksum.overflowing_add(value);
        checksum = sum + overflow as u16;
    };

    0b1111111111111111 ^ checksum
}

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
