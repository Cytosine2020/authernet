use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, Sender, SyncSender, SendError},
};
use rand::{Rng, thread_rng};
use crate::{
    mac::{MacFrame, MacAddress},
    rtaudio::{Stream, create_input_stream, create_output_stream},
    physical::{modulate, Demodulator},
};


const ACK_TIMEOUT: usize = 1100;
const BACK_OFF_WINDOW: usize = 500;
const FRAME_INTERVAL: usize = 50;


enum SendState<I> {
    Idle(usize),
    Sending(MacFrame, I, usize),
    WaitAck(MacFrame, usize, usize),
}

pub struct AthernetReceiver {
    receiver: Receiver<MacFrame>,
    _stream: Stream,
}

impl AthernetReceiver {
    pub fn new(receiver: Receiver<MacFrame>, _stream: Stream) -> Self {
        Self { receiver, _stream }
    }

    pub fn recv(&self) -> Result<MacFrame, RecvError> { self.receiver.recv() }
}

pub struct AthernetSender {
    sender: SyncSender<MacFrame>,
    _stream: Stream,
}

impl AthernetSender {
    pub fn new(sender: SyncSender<MacFrame>, _stream: Stream) -> Self {
        Self { sender, _stream }
    }

    pub fn send(&self, data: MacFrame) -> Result<(), SendError<MacFrame>> {
        self.sender.send(data)
    }
}

fn create_send_stream(
    mac_addr: u8,
    guard: Arc<AtomicBool>,
    ack_send_receiver: Receiver<(u8, u8)>,
    ack_recv_receiver: Receiver<(u8, u8)>,
    perf: bool,
) -> Result<AthernetSender, Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::sync_channel(0);

    let mut send_state = SendState::Idle(0);
    let mut buffer: Option<(MacFrame, usize, usize)> = None;

    let sending = move |frame: MacFrame, count| {
        SendState::Sending(frame, modulate(frame), count)
    };

    let backoff = move |frame: MacFrame, count: usize| {
        let maximum = 1 << std::cmp::min(4, count);
        let back_off = if frame.is_data() {
            thread_rng().gen_range::<usize, usize, usize>(0, maximum)
        } else {
            0
        };

        Some((frame, back_off * BACK_OFF_WINDOW, count))
    };

    let mut bit_count = 0;
    let mut time = std::time::SystemTime::now();

    let stream = create_output_stream(move |data: &mut [i16]| {
        let channel_free = guard.load(Ordering::SeqCst);

        if let Some((_, ref mut time, _)) = buffer {
            *time = time.saturating_sub(data.len());
        };

        match send_state {
            SendState::Idle(ref mut time) => {
                *time = time.saturating_sub(data.len());

                if *time == 0 && channel_free {
                    if let Some((dest, tag)) = ack_send_receiver.try_iter().next() {
                        send_state = sending(MacFrame::new_ack(mac_addr, dest, tag), 0);
                    } else if let Some((frame, time, count)) = buffer {
                        if time == 0 {
                            send_state = sending(frame, count + 1);
                            buffer = None;
                        };
                    } else if let Some(frame) = receiver.try_iter().next() {
                        send_state = sending(frame, 0);
                    };
                } else {
                    for _ in ack_send_receiver.try_iter() {}
                }
            }
            SendState::Sending(frame, ref mut iter, count) => {
                if channel_free {
                    for sample in data.iter_mut() {
                        if let Some(item) = iter.next() {
                            *sample = item;
                        } else {
                            send_state = if frame.is_data() && !frame.to_broadcast() {
                                SendState::WaitAck(frame, ACK_TIMEOUT, count)
                            } else {
                                SendState::Idle(0)
                            };
                            break;
                        };
                    };
                } else {
                    if frame.is_data() {
                        buffer = backoff(frame, count);
                    } else if !frame.is_ack() {
                        buffer = Some((frame, 0, count));
                    }
                    send_state = SendState::Idle(0);
                };
            }
            SendState::WaitAck(frame, ref mut time, count) => {
                *time = time.saturating_sub(data.len());

                if *time > 0 {
                    if ack_recv_receiver.try_iter().any(|item| {
                        item == (frame.get_dest(), frame.get_tag())
                    }) {
                        bit_count += frame.get_payload_size() * 8;

                        send_state = SendState::Idle(FRAME_INTERVAL);
                    };
                } else {
                    buffer = backoff(frame, count);
                    send_state = SendState::Idle(0);
                };
            }
        }

        if perf && time.elapsed().unwrap() > std::time::Duration::from_secs(1) {
            time = std::time::SystemTime::now();
            println!("speed {} b/s", bit_count);
            bit_count = 0;
        }
    })?;

    Ok(AthernetSender::new(sender, stream))
}

fn create_receive_stream(
    mac_addr: u8,
    guard: Arc<AtomicBool>,
    ack_send_sender: Sender<(u8, u8)>,
    ack_recv_sender: Sender<(u8, u8)>,
) -> Result<AthernetReceiver, Box<dyn std::error::Error>>
{
    let mut demodulator = Demodulator::new(mac_addr);

    let (sender, receiver) = mpsc::channel();

    let mut channel_active = false;

    let stream = create_input_stream(move |data: &mut [i16]| {
        for sample in data.iter() {
            if let Some(frame) = demodulator.push_back(*sample) {
                if frame.check(mac_addr) {
                    let tag = (frame.get_src(), frame.get_tag());

                    match frame.get_op() {
                        MacFrame::OP_ACK => {
                            ack_recv_sender.send(tag).unwrap();
                        }
                        MacFrame::OP_DATA => {
                            ack_send_sender.send(tag).unwrap();
                            sender.send(frame).unwrap();
                        }
                        _ => {}
                    }
                }
            }
        }

        if channel_active != demodulator.is_active() {
            channel_active = demodulator.is_active();
            guard.store(!channel_active, Ordering::SeqCst);
        }
    })?;

    Ok(AthernetReceiver::new(receiver, stream))
}

pub fn athernet_channel(mac_addr: u8, perf: bool)
                        -> Result<(AthernetReceiver, AthernetSender), Box<dyn std::error::Error>>
{
    let channel_free = Arc::new(AtomicBool::new(true));
    let (ack_send_send, ack_send_recv) = mpsc::channel::<(u8, u8)>();
    let (ack_recv_send, ack_recv_recv) = mpsc::channel::<(u8, u8)>();

    let receiver = create_receive_stream(
        mac_addr, channel_free.clone(), ack_send_send, ack_recv_send,
    )?;
    let sender = create_send_stream(
        mac_addr, channel_free.clone(), ack_send_recv, ack_recv_recv, perf,
    )?;

    Ok((receiver, sender))
}

pub struct MacLayerSender {
    athernet_sender: AthernetSender,
    send_tag: [u8; 255],
    mac_addr: u8,
}

impl MacLayerSender {
    pub fn new(athernet_send: AthernetSender, mac_addr: u8) -> Self {
        Self { athernet_sender: athernet_send, send_tag: [0u8; 255], mac_addr }
    }

    pub fn send(&mut self, data: &[u8], dest: MacAddress) -> Result<(), Box<dyn std::error::Error>> {
        let send_tag = &mut self.send_tag[dest as usize];

        let tag = if dest == MacFrame::BROADCAST_MAC {
            0
        } else {
            let tag = *send_tag;
            *send_tag = send_tag.wrapping_add(1);
            tag
        };

        Ok(self.athernet_sender.send(MacFrame::new_data(self.mac_addr, dest, tag, data))?)
    }
}

pub struct MacLayerReceiver {
    athernet_receiver: AthernetReceiver,
    recv_tag: [u8; 255],
}

impl MacLayerReceiver {
    pub fn new(athernet_receiver: AthernetReceiver) -> Self {
        Self { athernet_receiver, recv_tag: [0u8; 255] }
    }

    pub fn recv(&mut self, dest: MacAddress) -> Result<Box<[u8]>, Box<dyn std::error::Error>> {
        loop {
            let mac_data = self.athernet_receiver.recv()?;
            let src = mac_data.get_src();
            let tag = mac_data.get_tag();
            let recv_tag = &mut self.recv_tag[src as usize];

            if (src, tag) == (dest, *recv_tag & 0b1111) {
                *recv_tag = recv_tag.wrapping_add(1);
                return Ok(mac_data.unwrap());
            }
        }
    }
}

pub fn mac_channel(mac_addr: MacAddress, perf: bool)
                   -> Result<(MacLayerReceiver, MacLayerSender), Box<dyn std::error::Error>>
{
    let (athernet_receiver, athernet_sender) = athernet_channel(mac_addr, perf)?;

    let receiver = MacLayerReceiver::new(athernet_receiver);
    let sender = MacLayerSender::new(athernet_sender, mac_addr);

    Ok((receiver, sender))
}
