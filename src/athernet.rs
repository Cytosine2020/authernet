use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, Sender, SendError, RecvTimeoutError},
};
use rand::{Rng, thread_rng};
use crate::{
    mac::MacFrame, module::{Demodulator, modulate},
    rtaudio::{Stream, create_input_stream, create_output_stream},
};


const ACK_TIMEOUT: usize = 1200;
const BACK_OFF_WINDOW: usize = 64;
const FRAME_INTERVAL: usize = 64;


enum SendState<I> {
    Idle(usize),
    Sending(MacFrame, I, usize),
    WaitAck(MacFrame, usize, usize),
}

pub struct Athernet {
    sender: Sender<MacFrame>,
    receiver: Receiver<MacFrame>,
    ping_receiver: Receiver<(u8, u8)>,
    _input_stream: Stream,
    _output_stream: Stream,
}

impl Athernet {
    fn create_send_stream(
        mac_addr: u8,
        guard: Arc<AtomicBool>,
        ack_send_receiver: Receiver<(u8, u8)>,
        ack_recv_receiver: Receiver<(u8, u8)>,
        ping_receiver: Receiver<(u8, u8)>,
        perf: bool,
    ) -> Result<(Sender<MacFrame>, Stream), Box<dyn std::error::Error>> {
        let (sender, receiver) = mpsc::channel();

        let mut send_state = SendState::Idle(0);
        let mut buffer: Option<(MacFrame, usize, usize)> = None;

        let sending = move |frame: MacFrame, count| {
            // let tag = (frame.get_dest(), frame.get_tag());
            //
            // match frame.get_op() {
            //     MacFrame::OP_ACK => println!("sending ACK {:?}", tag),
            //     MacFrame::OP_DATA => println!("sending DATA {:?}", tag),
            //     MacFrame::OP_PING_REQ => println!("sending PING REQ {:?}", tag),
            //     MacFrame::OP_PING_REPLY => println!("sending RING REPLY {:?}", tag),
            //     _ => {}
            // }

            SendState::Sending(frame, modulate(frame), count)
        };

        let back_off = move |frame: MacFrame, count: usize| {
            let back_off = thread_rng().gen_range::<usize, usize, usize>(0, 16) +
                (1 << std::cmp::min(3, count));

            // println!("back off {:?}", (frame.get_dest(), frame.get_tag(), back_off));

            Some((frame, back_off * BACK_OFF_WINDOW, count))
        };

        let mut bit_count = 0;
        let mut time = std::time::SystemTime::now();

        let stream = create_output_stream(move |data: &mut [i16]| {
            let channel_free = guard.load(Ordering::SeqCst);

            if let Some((_, ref mut time, _)) = buffer {
                *time = time.saturating_sub(data.len());
            }

            match send_state {
                SendState::Idle(ref mut time) => {
                    *time = time.saturating_sub(data.len());
                }
                SendState::WaitAck(_, ref mut time, _) => {
                    *time = time.saturating_sub(data.len());
                }
                _ => {}
            }

            for sample in data.iter_mut() {
                match send_state {
                    SendState::Idle(time) => {
                        if channel_free && time == 0 {
                            if let Some((dest, tag)) = ack_send_receiver.try_iter().next() {
                                let frame = MacFrame::new_ack(mac_addr, dest, tag);
                                send_state = sending(frame, 0);
                            } else if let Some((dest, tag)) = ping_receiver.try_iter().next() {
                                let frame = MacFrame::new_ping_reply(mac_addr, dest, tag);
                                send_state = sending(frame, 0);
                            } else if let Some((frame, time, count)) = buffer {
                                if time == 0 {
                                    send_state = sending(frame, count + 1);
                                    buffer = None;
                                } else {
                                    break;
                                }
                            } else if let Some(frame) = receiver.try_iter().next() {
                                send_state = sending(frame, 0);
                            } else {
                                break;
                            };
                        } else {
                            break;
                        };
                    }
                    SendState::Sending(frame, ref mut iter, count) => {
                        if channel_free {
                            if let Some(item) = iter.next() {
                                *sample = item;
                            } else {
                                send_state = if frame.is_data() && !frame.to_broadcast() {
                                    SendState::WaitAck(frame, ACK_TIMEOUT, count)
                                } else {
                                    SendState::Idle(0)
                                }
                            }
                        } else {
                            // println!("collision");
                            if !frame.is_ack() {
                                buffer = back_off(frame, count);
                            }
                            send_state = SendState::Idle(0);
                        };
                    }
                    SendState::WaitAck(frame, ref mut time, count) => {
                        if *time > 0 {
                            if ack_recv_receiver.try_iter().any(|item| {
                                item == (frame.get_dest(), frame.get_tag())
                            }) {
                                bit_count += frame.get_payload_size() * 8;

                                send_state = SendState::Idle(FRAME_INTERVAL);
                            } else {
                                break;
                            };
                        } else {
                            // println!("retransmit");
                            buffer = back_off(frame, count);
                            send_state = SendState::Idle(0);
                        };
                    }
                };
            };

            if perf && time.elapsed().unwrap() > std::time::Duration::from_secs(1) {
                time = std::time::SystemTime::now();
                println!("speed {} b/s", bit_count);
                bit_count = 0;
            }
        })?;

        Ok((sender, stream))
    }

    fn create_receive_stream(
        mac_addr: u8,
        guard: Arc<AtomicBool>,
        ack_send_sender: Sender<(u8, u8)>,
        ack_recv_sender: Sender<(u8, u8)>,
        ping_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<MacFrame>, Receiver<(u8, u8)>, Stream), Box<dyn std::error::Error>>
    {
        let mut demodulator = Demodulator::new(mac_addr);

        let (sender, receiver) = mpsc::channel();
        let (ping_send, ping_recv) = mpsc::channel();

        let mut channel_active = false;

        let stream = create_input_stream(move |data: &mut [i16]| {
            for sample in data.iter() {
                if let Some(frame) = demodulator.push_back(*sample) {
                    if frame.check(mac_addr) {
                        let tag = (frame.get_src(), frame.get_tag());

                        match frame.get_op() {
                            MacFrame::OP_ACK => {
                                ack_recv_sender.send(tag).unwrap();
                                // println!("receiving ACK {:?}", tag);
                            }
                            MacFrame::OP_DATA => {
                                ack_send_sender.send(tag).unwrap();
                                sender.send(frame).unwrap();
                                // println!("receiving DATA {:?}", tag);
                            }
                            MacFrame::OP_PING_REQ => {
                                ping_sender.send(tag).unwrap();
                                // println!("receiving PING REQ {:?}", tag);
                            }
                            MacFrame::OP_PING_REPLY => {
                                ping_send.send(tag).unwrap();
                                // println!("receiving PING REPLY {:?}", tag);
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

        Ok((receiver, ping_recv, stream))
    }

    pub fn new(mac_addr: u8, perf: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let channel_free = Arc::new(AtomicBool::new(true));
        let (ack_send_send, ack_send_recv) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_send, ack_recv_recv) = mpsc::channel::<(u8, u8)>();
        let (ping_send, ping_recv) = mpsc::channel::<(u8, u8)>();

        let (receiver, ping_receiver, _output_stream)
            = Self::create_receive_stream(
            mac_addr, channel_free.clone(), ack_send_send, ack_recv_send, ping_send,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            mac_addr, channel_free.clone(), ack_send_recv, ack_recv_recv, ping_recv, perf,
        )?;

        Ok(Self { sender, receiver, ping_receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: MacFrame) -> Result<(), SendError<MacFrame>> {
        self.sender.send(data)
    }

    pub fn recv(&self) -> Result<MacFrame, RecvError> { self.receiver.recv() }

    pub fn ping_recv_timeout(&self, timeout: std::time::Duration)
                             -> Result<(u8, u8), RecvTimeoutError>
    {
        self.ping_receiver.recv_timeout(timeout)
    }
}
