pub mod ipv4;
pub mod udp;


use std::fmt::Formatter;
use crate::athernet::MacLayer;


pub struct IPV4Datagram(Box<[u8]>);

impl std::fmt::Debug for IPV4Datagram {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let ip_header = &ipv4::Header::from_slice(&*self.0);

        f.debug_struct("IPV4Datagram")
            .field("header", &ip_header)
            .field("data", &&self.0[ip_header.get_header_length()..])
            .finish()
    }
}

pub struct IPV4FragmentIter<'a> {
    builder: &'a IPV4DatagramBuilder,
    protocol: u8,
    data: Box<[u8]>,
    offset: usize,
}

impl<'a> IPV4FragmentIter<'a> {
    pub fn new(builder: &'a IPV4DatagramBuilder, protocol: u8, data: Box<[u8]>) -> Self {
        Self { builder, protocol, data, offset: 0 }
    }
}

impl<'a> Iterator for IPV4FragmentIter<'a> {
    type Item = IPV4Datagram;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset < self.data.len() {
            let ip_header = self.builder.create_ip_header(
                self.protocol, self.offset, self.data.len() - self.offset,
            );

            let total_len = ip_header.get_total_length();
            let header_len = ip_header.get_header_length();
            let data_len = ip_header.get_payload_length();

            let mut datagram = std::iter::repeat(0).take(total_len).collect::<Box<_>>();

            datagram[..header_len].copy_from_slice(ip_header.get_slice());
            datagram[header_len..].copy_from_slice(&self.data[self.offset..][..data_len]);

            self.offset += data_len;

            Some(IPV4Datagram(datagram))
        } else {
            None
        }
    }
}


pub struct IPV4DatagramBuilder {
    mtu: usize,
    identification: Option<u16>,
    do_not_fragment: bool,
    time_to_live: Option<u8>,
    src_ip: Option<ipv4::Address>,
    dest_ip: Option<ipv4::Address>,
    src_port: Option<u16>,
    dest_port: Option<u16>,
}

impl IPV4DatagramBuilder {
    pub fn new(mtu: usize) -> Self {
        Self {
            mtu,
            identification: None,
            do_not_fragment: true,
            time_to_live: None,
            src_ip: None,
            dest_ip: None,
            src_port: None,
            dest_port: None,
        }
    }

    pub fn set_identification(&mut self, identification: u16) -> &mut Self {
        self.identification = Some(identification);
        self
    }

    pub fn enable_fragment(&mut self) -> &mut Self {
        self.do_not_fragment = true;
        self
    }

    pub fn set_time_to_live(&mut self, time_to_live: u8) -> &mut Self {
        self.time_to_live = Some(time_to_live);
        self
    }

    pub fn set_src_ip(&mut self, ip: ipv4::Address) -> &mut Self {
        self.src_ip = Some(ip);
        self
    }

    pub fn set_dest_ip(&mut self, ip: ipv4::Address) -> &mut Self {
        self.dest_ip = Some(ip);
        self
    }

    pub fn set_src_port(&mut self, port: u16) -> &mut Self {
        self.src_port = Some(port);
        self
    }

    pub fn set_dest_port(&mut self, port: u16) -> &mut Self {
        self.dest_port = Some(port);
        self
    }

    pub fn set_src(&mut self, ip: ipv4::Address, port: u16) -> &mut Self {
        self.src_ip = Some(ip);
        self.src_port = Some(port);
        self
    }

    pub fn set_dest(&mut self, ip: ipv4::Address, port: u16) -> &mut Self {
        self.dest_ip = Some(ip);
        self.dest_port = Some(port);
        self
    }

    pub fn create_ip_header(
        &self, protocol: u8, offset: usize, payload_size: usize,
    ) -> ipv4::Header {
        ipv4::Header::new(
            0,
            self.mtu, payload_size,
            self.identification.unwrap(),
            self.do_not_fragment, offset as u16,
            self.time_to_live.unwrap(), protocol,
            self.src_ip.unwrap(),
            self.dest_ip.unwrap(),
        )
    }

    pub fn create_udp_header(&self, payload_length: usize) -> udp::Header {
        udp::Header::new(
            self.src_port.unwrap(),
            self.dest_port.unwrap(),
            payload_length,
        )
    }

    pub fn finalize_udp_with_data(&self, data: &[u8]) -> IPV4FragmentIter {
        let udp_header = self.create_udp_header(data.len());

        let payload = udp_header.get_slice().iter()
            .chain(data.iter()).cloned().collect::<Box<_>>();

        IPV4FragmentIter::new(self, udp::PROTOCOL, payload)
    }
}


pub struct IPV4Layer {
    ip_address: ipv4::Address,
    mac_layer: MacLayer,
}

impl IPV4Layer {
    pub fn new(
        ip_address: ipv4::Address, mac_address: u8,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { ip_address, mac_layer: MacLayer::new(mac_address, false)? })
    }

    pub fn send(
        &mut self, data: &[u8], dest: ipv4::Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut builder = IPV4DatagramBuilder::new(self.mac_layer.get_mtu());

        builder
            .set_identification(1000)
            .set_time_to_live(20)
            .set_src(self.ip_address, 40001)
            .set_dest(dest, 40002);

        for item in builder.finalize_udp_with_data(&data) {
            self.mac_layer.send(&*item.0, 5)?;
        }

        Ok(())
    }

    pub fn recv(&mut self) -> Result<Box<[u8]>, Box<dyn std::error::Error>> {
        self.mac_layer.recv(5)
    }
}
