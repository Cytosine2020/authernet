use crate::DATA_PACK_SIZE;
use crate::bit_set::DataPack;
use std::{
    env, fs::File, cmp::min, mem::size_of,
    io::{Read, Write, BufWriter, BufReader, Error},
};
const CRC_8_Gen:u32=0x131;
pub struct File_read{
    CRC_8_table:[u8;256],
    buf_read:BufReader<File>,
    size:u64,
    num:u8,
    count:u8,
}
impl File_read{
    pub fn new(file:File) -> Result<Self,Box<dyn std::error::Error>>{
        let size=file.metadata()?.len() as u64;
        let buf_read=BufReader::new(file);
        let CRC_8_table:[u8;256]=create_crc_table();
        Ok(Self{
            CRC_8_table:CRC_8_table,
            buf_read:buf_read,
            size:size,
            num:((10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE) as u8,
            count:0,
        })
    }
    fn crc_generate(&self,data:DataPack) -> DataPack {
        let mut crc:u8=0;
        let mut ret:DataPack=[0;DATA_PACK_SIZE/8];
        let CRC_8_table:&[u8;256]=&self.CRC_8_table;
        for i in 0..(DATA_PACK_SIZE-8)/8  {
            crc=crc ^ data[i];
            crc=CRC_8_table[crc as usize];
        }
        ret[..(DATA_PACK_SIZE-16)/8].copy_from_slice(&data[..(DATA_PACK_SIZE-16)/8]);
        ret[(DATA_PACK_SIZE-8)/8]=crc;
        ret
    }
    fn create_crc_table(self) -> [u8;256]{
        let mut j:[u8;256]=[0;256];
        let mut crc:u8=0;
        for i in 0..0xFF{
            crc=i;
            for q in 0..=8{
                if i&0x80 > 0{
                    crc = (crc << 1) ^ 0x31;
                }else{
                    crc = (crc << 1);
                }
            }
            j[i as usize]=crc;
        }
        j
    }
}
fn create_crc_table() -> [u8;256]{
    let mut j:[u8;256]=[0;256];
    let mut crc:u8=0;
    for i in 0..0xFF{
        crc=i;
        for q in 0..=8{
            if i&0x80 > 0{
                crc = (crc << 1) ^ 0x31;
            }else{
                crc = (crc << 1);
            }
        }
        j[i as usize]=crc;
    }
    j
}
impl Iterator for File_read{
    type Item = DataPack;


    fn next(&mut self) -> Option<DataPack>{
        if self.size <= 0{
            return None;
        }
        let index:usize=f32::log2((self.num*2-1) as f32) as usize;
        let mut buf:[u8;DATA_PACK_SIZE-8]=[0;DATA_PACK_SIZE-8];
        let mut ret:DataPack=[0;DATA_PACK_SIZE/8];
        
        
        if self.size > (buf.len()-index) as u64{
            self.buf_read.read_exact(&mut buf[index..]);
            for i in 0..index{
                buf[i]=(self.count>>i&0x1)+30;
            }
            self.count+=1;
            self.size -= (buf.len()-index) as u64;
        }else{
            for i in 0..index{
                buf[i]=(self.count>>i&0x1)+30;
            }
            self.count+=1;
            self.buf_read.read_exact(&mut buf[index..self.size as usize]);
            self.size =0;
        }
        
    
        for i in 0..(ret.len()-1){
            for j in 0..8{
                let p=i*8+j;
                ret[i]+=(0x1&buf[p])<<j;
            }
        }
        ret=self.crc_generate(ret);
        Some(ret)

    }
}
pub struct File_write{
    file:File,
    num:[bool;(10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE],
    CRC_table:[u8;256],
    data:[u8;10000],
    point:usize,
    pub count:u8
}
impl File_write{
    pub fn new() -> Result<Self, Error>{
        let mut form_data:[u8;10000]=[0;10000];
        let mut num:[bool;(10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE]=[false;(10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE];
        let file=File::create("output.txt")?;
        let Crc=create_crc_table();
        Ok(Self{
            file:file,
            num:num,
            CRC_table:Crc,
            data:form_data,
            point:0,
            count:((10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE) as u8
        })
    }
    pub fn write_in(&mut self,data:DataPack) -> Result<(), Error>{
        let index:usize=f32::log2(((10000+DATA_PACK_SIZE-1)/DATA_PACK_SIZE*2-1) as f32) as usize;
        let mut buf:[u8;DATA_PACK_SIZE-8]=[0;DATA_PACK_SIZE-8];
        if self.crc_compare(&data){
            for i in 0..data.len()-1{
                for j in 0..8{
                    buf[i*8+j]=30+0x1&(data[i]>>j);
                }
            }
            let mut num:usize=0;
            for i in 0..index{
                num+=((buf[i]&0x1)<<i) as usize;
            }
            self.num[num]=true;
            self.point=(num)*(DATA_PACK_SIZE-8-index);
            let upper=min(self.point+(DATA_PACK_SIZE-8-index),10000);
            for i in index..upper{
                self.data[i+self.point]=buf[i];
            }
            self.count-=1;
            
        }
        
        Ok(())
    }
    pub fn write_allin(&mut self) -> Result<(),bool>{
        for i in self.num.iter(){
            if i==&false{
                return Err(false)
            }
        }
        self.file.write_all(&self.data);
        Ok(())
    }
    fn create_crc_table(&self) -> [u8;256]{
        let mut j:[u8;256]=[0;256];
        let mut crc:u8=0;
        for i in 0..0xFF{
            crc=i;
            for q in 0..=8{
                if i&0x80 > 0{
                    crc = (crc << 1) ^ 0x31;
                }else{
                    crc = (crc << 1);
                }
            }
            j[i as usize]=crc;
        }
        j
    }
    fn crc_compare(&self,data:&DataPack) -> bool{
        let mut crc:u8=0;
        let CRC_8_table:[u8;256]=self.create_crc_table();
        for i in 0..DATA_PACK_SIZE/8  {
            crc=crc ^ data[i];
            crc=CRC_8_table[crc as usize];
        }
        if crc > 0{
            false
        }else{
            true
        }
    }
}



