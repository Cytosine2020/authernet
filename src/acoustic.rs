use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, Sender, SendError, RecvTimeoutError},
};
use cpal::{
    Device, Host, Sample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rand::{Rng, thread_rng};
use crate::{mac::MacFrame, module::{Demodulator, modulate}};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 10000;
const BACK_OFF_WINDOW: usize = 128;
const DIFS: usize = 512;


fn select_host() -> Host { cpal::default_host() }

fn select_config<T: Iterator<Item=SupportedStreamConfigRange>>(
    config: T
) -> Result<SupportedStreamConfig, Box<dyn std::error::Error>> {
    Ok(config.map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .min_by_key(|item| item.channels())
        .ok_or("expected configuration not found")?)
}


enum SendState<I> {
    Idle,
    Sending(MacFrame, I, usize),
    WaitAck(MacFrame, usize, usize),
}

pub struct Athernet {
    sender: Sender<MacFrame>,
    receiver: Receiver<MacFrame>,
    ping_receiver: Receiver<(u8, u8)>,
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

impl Athernet {
    fn create_send_stream(
        mac_addr: u8,
        device: Device,
        guard: Arc<AtomicBool>,
        ack_send_receiver: Receiver<(u8, u8)>,
        ack_recv_receiver: Receiver<(u8, u8)>,
        ping_receiver: Receiver<(u8, u8)>,
        perf: bool,
    ) -> Result<(Sender<MacFrame>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_output_configs()?)?;

        let channel = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut send_state = SendState::Idle;
        let mut buffer: Option<(MacFrame, usize, usize)> = None;

        let sending = move |frame: MacFrame, count| {
            let iter = std::iter::repeat(0).take(DIFS).chain(modulate(frame));

            SendState::Sending(frame, iter.map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten(), count)
        };

        let back_off = move |frame: MacFrame, count: usize| {
            // if count <= 20 {
                let back_off = thread_rng().gen_range::<usize, usize, usize>(0, 4) +
                    1 << std::cmp::min(4, count);
                Some((frame, back_off * BACK_OFF_WINDOW, count))
            // } else {
            //     println!("package loss {:?}", (frame.get_dest(), frame.get_tag()));
            //
            //     None
            // }
        };

        let mut bit_count = 0;
        let mut time = std::time::SystemTime::now();

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let channel_free = guard.load(Ordering::SeqCst);

                if let Some((_, ref mut time, _)) = buffer {
                    *time = time.saturating_sub(data.len());
                }

                if let SendState::WaitAck(_, ref mut time, _) = send_state {
                    *time = time.saturating_sub(data.len());
                }

                for sample in data.iter_mut() {
                    *sample = 0.;
                }

                for sample in data.iter_mut() {
                    match send_state {
                        SendState::Idle => {
                            if let Some((dest, tag)) = ack_send_receiver.try_iter().next() {
                                let frame = MacFrame::new_ack(mac_addr, dest, tag);
                                send_state = sending(frame, 0);
                            } else if let Some((dest, tag)) = ping_receiver.try_iter().next() {
                                let frame = MacFrame::new_ping_reply(mac_addr, dest, tag);
                                send_state = sending(frame, 0);
                            } else if channel_free {
                                if let Some((frame, time, count)) = buffer {
                                    if time == 0 {
                                        buffer = None;
                                        send_state = sending(frame, count + 1);
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
                            if channel_free || !frame.is_data() {
                                if let Some(item) = iter.next() {
                                    *sample = Sample::from(&item);
                                } else {
                                    send_state = if frame.is_data() && !frame.to_broadcast() {
                                        SendState::WaitAck(frame, ACK_TIMEOUT, count)
                                    } else {
                                        SendState::Idle
                                    }
                                }
                            } else {
                                buffer = back_off(frame, count);
                                send_state = SendState::Idle;
                            };
                        }
                        SendState::WaitAck(frame, ref mut time, count) => {
                            if *time > 0 {
                                if ack_recv_receiver.try_iter().any(|item| {
                                    item == (frame.get_dest(), frame.get_tag())
                                }) {
                                    bit_count += frame.get_payload_size() * 8;

                                    send_state = SendState::Idle;
                                } else {
                                    break;
                                };
                            } else {
                                buffer = back_off(frame, count);
                                send_state = SendState::Idle;
                            };
                        }
                    };
                };

                if perf && time.elapsed().unwrap() > std::time::Duration::from_secs(1) {
                    time = std::time::SystemTime::now();
                    println!("speed {} b/s", bit_count);
                    bit_count = 0;
                }
            },
            |err| {
                eprintln!("an error occurred on the output audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((sender, stream))
    }

    fn create_receive_stream(
        mac_addr: u8,
        device: Device,
        guard: Arc<AtomicBool>,
        ack_send_sender: Sender<(u8, u8)>,
        ack_recv_sender: Sender<(u8, u8)>,
        ping_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<MacFrame>, Receiver<(u8, u8)>, cpal::Stream), Box<dyn std::error::Error>>
    {
        let config = select_config(device.supported_input_configs()?)?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();
        let (ping_send, ping_recv) = mpsc::channel();

        let mut channel = 0;
        let mut channel_active = false;

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if channel == 0 {
                        if let Some(frame) = demodulator.push_back(Sample::from(sample)) {
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
                                    MacFrame::OP_PING_REQ => {
                                        ping_sender.send(tag).unwrap();
                                    }
                                    MacFrame::OP_PING_REPLY => {
                                        ping_send.send(tag).unwrap();
                                    }
                                    _ => {}
                                }
                            }
                        }

                        if channel_active != demodulator.is_active() {
                            channel_active = demodulator.is_active();
                            guard.store(!channel_active, Ordering::SeqCst);
                        }
                    }

                    channel += 1;
                    if channel == channel_count { channel = 0; }
                }
            },
            |err| {
                eprintln!("an error occurred on the inout audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((receiver, ping_recv, stream))
    }

    pub fn new(mac_addr: u8, perf: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let host = select_host();

        let channel_free = Arc::new(AtomicBool::new(true));
        let (ack_send_send, ack_send_recv) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_send, ack_recv_recv) = mpsc::channel::<(u8, u8)>();
        let (ping_send, ping_recv) = mpsc::channel::<(u8, u8)>();

        let (receiver, ping_receiver, _output_stream)
            = Self::create_receive_stream(
            mac_addr, host.default_input_device().ok_or("no input device available!")?,
            channel_free.clone(), ack_send_send, ack_recv_send, ping_send,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            mac_addr, host.default_output_device().ok_or("no output device available!")?,
            channel_free.clone(), ack_send_recv, ack_recv_recv, ping_recv, perf
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
