use std::env;
use crate::athernet::mac_channel;


mod athernet;
mod physical;
mod rtaudio;
mod mac;

#[macro_use]
extern crate lazy_static;


const ATHERNET_SOCKET: &str = "/tmp/athernet.socket";


fn open_socket() -> i32 {
    let mut addr = libc::sockaddr_un {
        sun_len: 0,
        sun_family: libc::AF_UNIX as u8,
        sun_path: [0; 104],
    };

    let str_size = std::cmp::min(ATHERNET_SOCKET.len(), addr.sun_path.len() - 1);
    let size = std::mem::size_of::<libc::sockaddr_un>();

    let socket = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if socket < 0 { panic!("socket error"); }

    println!("{}", socket);

    unsafe {
        addr.sun_path[..str_size].copy_from_slice(
            std::slice::from_raw_parts(ATHERNET_SOCKET.as_bytes().as_ptr() as *mut _, str_size)
        )
    };

    if unsafe { libc::bind(socket, &addr as *const _ as *const _, size as u32) } != 0 {
        panic!("bind error");
    }

    if unsafe { libc::listen(socket, 0) } != 0 { panic!("listen error"); }

    let client = unsafe {
        libc::accept(socket, std::ptr::null_mut() as *mut _, std::ptr::null_mut() as *mut _)
    };
    if client < 0 { panic!("accept error {}", std::io::Error::last_os_error()); }

    unsafe { libc::close(socket) };

    client
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();

    args.next();

    let mac_addr = args.next().unwrap().parse::<u8>()? & 0b1111;
    let dest = args.next().unwrap().parse::<u8>()? & 0b1111;

    let (mut receiver, mut sender) = mac_channel(mac_addr, false)?;

    let client = open_socket();

    std::thread::spawn(move || {
        let mut buffer = [0u8; 2048];

        loop {
            let size = unsafe {
                libc::recv(client, buffer.as_mut_ptr() as *mut _, buffer.len(), 0)
            };

            if size < 0 { panic!("recv error"); }

            sender.send(&buffer[..size as usize], dest).unwrap();
        }
    });

    loop {
        let buffer = receiver.recv(dest).unwrap();

        let size = unsafe {
            libc::send(client, buffer.as_ptr() as *const _, buffer.len(), 0)
        };

        if size as usize != buffer.len() { panic!("send error"); }
    }
}
