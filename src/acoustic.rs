use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, Sender, SendError},
};
use cpal::{
    Device, Host, Sample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rand::{Rng, thread_rng};
use crate::{mac::MacFrame, module::{Demodulator, Modulator}};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 12000;
const BACK_OFF_WINDOW: usize = 500;


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
    ) -> Result<(Sender<MacFrame>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_output_configs()?)?;

        let modulator = Modulator::new();

        let channel = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut send_state = SendState::Idle;
        let mut back_off_buffer = None;

        let sending = move |buffer: MacFrame, count| {
            // let mac_data = MacData::copy_from_slice(&buffer);
            // let tag = (mac_data.get_dest(), mac_data.get_index());
            //
            // match mac_data.get_op() {
            //     MacData::DATA => println!("sending data {:?}", tag),
            //     MacData::ACK => println!("sending ack {:?}", tag),
            //     _ => {}
            // }

            SendState::Sending(buffer, modulator.iter(buffer).map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten(), count)
        };

        let back_off = move |buffer: MacFrame, count: usize| {
            let tag = (buffer.get_dest(), buffer.get_tag());

            if count <= 20 {
                let back_off = thread_rng().gen_range::<usize, usize, usize>(0, 16) +
                    if count > 5 { 1 << 5 } else { 1 << count };

                // println!("back off {:?}", (tag, back_off));

                Some((buffer, back_off * BACK_OFF_WINDOW, count))
            } else {
                println!("package loss {:?}", tag);

                None
            }
        };

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let channel_free = guard.load(Ordering::SeqCst);

                let ack_recv = ack_recv_receiver.try_iter().collect::<Vec<_>>();
                let mut ack_send = ack_send_receiver.try_iter().collect::<Vec<_>>()
                    .into_iter();

                for sample in data.iter_mut() {
                    let mut value = 0;

                    if let Some((_, ref mut time, _)) = back_off_buffer {
                        if *time > 0 { *time -= 1 }
                    }

                    match send_state {
                        SendState::Idle => {
                            if channel_free {
                                if let Some((dest, tag)) = ack_send.next() {
                                    send_state = sending(MacFrame::new_ack(mac_addr, dest, tag), 0);
                                } else if let Some((buffer, time, count)) = back_off_buffer {
                                    if time == 0 {
                                        back_off_buffer = None;
                                        send_state = sending(buffer, count + 1);
                                    }
                                } else if let Some(buffer) = receiver.try_iter().next() {
                                    send_state = sending(buffer, 0);
                                } else {
                                    send_state = SendState::Idle;
                                }
                            };
                        }
                        SendState::Sending(buffer, ref mut iter, count) => {
                            if channel_free {
                                if let Some(item) = iter.next() {
                                    value = item;
                                } else {
                                    send_state = if buffer.need_ack() {
                                        SendState::WaitAck(buffer, ACK_TIMEOUT, count)
                                    } else {
                                        SendState::Idle
                                    }
                                }
                            } else {
                                if buffer.need_back_off() {
                                    back_off_buffer = back_off(buffer, count);
                                }
                                send_state = SendState::Idle;
                            }
                        }
                        SendState::WaitAck(buffer, ref mut time, count) => {
                            if *time > 0 {
                                let tag = (buffer.get_dest(), buffer.get_tag());

                                if ack_recv.iter().any(|item| *item == tag) {
                                    send_state = SendState::Idle;
                                } else {
                                    *time -= 1;
                                }
                            } else {
                                back_off_buffer = back_off(buffer, count);
                                send_state = SendState::Idle;
                            };
                        }
                    }

                    *sample = Sample::from(&value);
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
    ) -> Result<(Receiver<MacFrame>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_input_configs()?)?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut channel = 0;
        let mut channel_active = false;

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if channel == 0 {
                        if let Some(buffer) = demodulator.push_back(Sample::from(sample)) {
                            if buffer.check(mac_addr) {
                                let tag = (buffer.get_src(), buffer.get_tag());
                                match buffer.get_op() {
                                    MacFrame::OP_ACK => {
                                        // println!("receive ack {:?}", tag);

                                        ack_recv_sender.send(tag).unwrap();
                                    }
                                    MacFrame::OP_DATA => {
                                        // println!("receive data {:?}", tag);

                                        ack_send_sender.send(tag).unwrap();

                                        sender.send(buffer).unwrap();
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

        Ok((receiver, stream))
    }

    pub fn new(mac_addr: u8) -> Result<Self, Box<dyn std::error::Error>> {
        let channel_free = Arc::new(AtomicBool::new(true));
        let host = select_host();

        let (ack_send_sender, ack_send_receiver) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_sender, ack_recv_receiver) = mpsc::channel::<(u8, u8)>();

        let (receiver, _output_stream) = Self::create_receive_stream(
            mac_addr.clone(), host.default_input_device().ok_or("no input device available!")?,
            channel_free.clone(), ack_send_sender, ack_recv_sender,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            mac_addr, host.default_output_device().ok_or("no output device available!")?,
            channel_free.clone(), ack_send_receiver, ack_recv_receiver,
        )?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: MacFrame) -> Result<(), SendError<MacFrame>> {
        self.sender.send(data)
    }

    pub fn recv(&self) -> Result<MacFrame, RecvError> { self.receiver.recv() }
}
