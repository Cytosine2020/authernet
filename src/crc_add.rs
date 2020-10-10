use crate::{DATA_PACK_SIZE, FILE_SIZE, bit_set::DataPack};
use std::{
    fs::File, cmp::min,
    io::{Read, Write},
};
use lazy_static;


const INDEX: usize = 8;
const PACKAGE_NUM: usize = (FILE_SIZE + (DATA_PACK_SIZE - 16) - 1) / (DATA_PACK_SIZE - 16);

lazy_static!(
    static ref CRC_TABLE: [u8; 256] = {
        let mut table = [0; 256];
        let mut crc;
        for i in 0..0xFF {
            crc = i;
            for _ in 0..=8 {
                if i & 0x80 > 0 {
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

pub struct FileRead {
    file: File,
    size: usize,
    count: u8,
}

impl FileRead {
    pub fn new(file: File) -> Self {
        Self {
            file,
            size: FILE_SIZE,
            count: 0,
        }
    }

    fn crc_generate(data: DataPack) -> DataPack {
        const DATA_SIZE: usize = (DATA_PACK_SIZE - 8) / 8;

        let mut crc = 0;
        let mut ret = [0; DATA_PACK_SIZE / 8];

        for i in 0..DATA_SIZE {
            crc = crc ^ data[i];
            crc = CRC_TABLE[crc as usize];
        }

        ret[..DATA_SIZE].copy_from_slice(&data[..DATA_SIZE]);
        ret[DATA_SIZE] = crc;

        ret
    }
}

impl Iterator for FileRead {
    type Item = DataPack;

    fn next(&mut self) -> Option<DataPack> {
        if self.size <= 0 { return None; }

        let mut buf = [0; DATA_PACK_SIZE - 8];
        let mut ret = [0; DATA_PACK_SIZE / 8];

        if self.size > (buf.len() - INDEX) {
            self.file.read_exact(&mut buf[INDEX..]).unwrap();

            for i in 0..INDEX {
                buf[i] = (self.count >> i & 0x1) + '0' as u8;
            }

            self.count += 1;
            self.size -= buf.len() - INDEX;
        } else {
            for i in 0..INDEX {
                buf[i] = (self.count >> i & 0x1) + '0' as u8;
            }

            self.count += 1;
            self.file.read_exact(&mut buf[INDEX..INDEX + self.size as usize]).unwrap();
            self.size = 0;
        }

        for i in 0..(ret.len() - 1) {
            for j in 0..8 {
                let p = i * 8 + j;
                ret[i] += (0x1 & buf[p]) << j;
            }
        }

        ret = Self::crc_generate(ret);

        Some(ret)
    }
}

pub struct FileWrite {
    file: File,
    num: [bool; PACKAGE_NUM],
    data: [u8; FILE_SIZE],
    point: usize,
    pub count: u8,
}

impl FileWrite {
    pub fn new(file: File) -> Self {
        Self {
            file,
            num: [false; PACKAGE_NUM],
            data: [0; FILE_SIZE],
            point: 0,
            count: PACKAGE_NUM as u8,
        }
    }

    pub fn write_in(&mut self, data: DataPack) {
        let mut buf = [0; DATA_PACK_SIZE - 8];

        if self.crc_compare(&data) {
            for i in 0..data.len() - 1 {
                for j in 0..8 {
                    buf[i * 8 + j] = '0' as u8 + (0x1 & (data[i] >> j));
                }
            }

            let mut num = 0;

            for i in 0..INDEX {
                num += ((buf[i] & 0x1) << i) as usize;
            }

            self.num[num] = true;
            self.point = num * (DATA_PACK_SIZE - 8 - INDEX);
            let upper = min(DATA_PACK_SIZE - 8 - INDEX, FILE_SIZE - self.point);

            for i in 0..upper {
                self.data[self.point + i] = buf[i + INDEX];
            }

            self.count -= 1;
        } else {
            println!("crc fail!");
        }
    }

    pub fn write_allin(&mut self) {
        self.file.write_all(&self.data).unwrap();
    }

    fn crc_compare(&self, data: &DataPack) -> bool {
        let mut crc = 0;

        for i in 0..DATA_PACK_SIZE / 8 {
            crc = crc ^ data[i];
            crc = CRC_TABLE[crc as usize];
        }

        crc == 0
    }
}
