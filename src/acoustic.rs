use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError},
};
use cpal::{Host, Sample, traits::{DeviceTrait, HostTrait, StreamTrait}};
use crate::{DataPack, module::{Modulator, Demodulator}};
use crate::acoustic::SendState::WaitAck;


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 256;
const BACK_OFF_WINDOW: usize = 256;

// #[cfg(target_os = "windows")]
// fn get_host() -> Host {
//     cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host")
// }
//
// #[cfg(target_os = "macos")]
fn get_host() -> Host { cpal::default_host() }

pub fn print_config() {
    let host = get_host();

    println!("Host: {:?}", host.id());

    if let Some(output_device) = host.default_output_device() {
        println!("Output device name: {:?}", output_device.name());

        for config in output_device
            .supported_output_configs().expect("error while querying configs")
            .map(|item| item.with_max_sample_rate()) {
            println!("{:#?}", config);
        }
    }

    if let Some(input_device) = host.default_input_device() {
        println!("Input device name: {:?}", input_device.name());

        for config in input_device
            .supported_input_configs().expect("error while querying configs")
            .map(|item| item.with_max_sample_rate()) {
            println!("{:#?}", config);
        }
    }
}

enum SendState<I> {
    Idle,
    Sending(DataPack, I),
    BackOff(DataPack, usize),
    WaitAck(DataPack, usize),
}

impl<I: Iterator<Item=i16>> Iterator for SendState<I> {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        if let Self::Sending(buffer, iter) = self {
            if let Some(item) = iter.next() {
                Some(item)
            } else {
                *self = WaitAck(*buffer, ACK_TIMEOUT);
                Some(0)
            }
        } else {
            Some(0)
        }
    }
}

pub struct Athernet {
    sender: Sender<DataPack>,
    receiver: Receiver<DataPack>,
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

impl Athernet {
    fn create_send_stream(
        guard: Arc<AtomicBool>,
        _ack_send_receiver: Receiver<(u8, u8)>,
        ack_rcev_receiver: Receiver<(u8, u8)>,
    ) -> Result<(Sender<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let device = get_host().default_output_device().ok_or("no input device available")?;

        let config = device.supported_output_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .min_by_key(|item| item.channels())
            .ok_or("expected configuration not found")?;

        let modulator = Modulator::new();

        let channel = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let message_signal = move |data| {
            SendState::Sending(data, modulator.iter(data).map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten())
        };

        let mut send_state = SendState::Idle;

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let channel_free = guard.load(Ordering::SeqCst);

                match send_state {
                    SendState::Idle => {
                        if channel_free {
                            match receiver.try_recv() {
                                Ok(buffer) => send_state = message_signal(buffer),
                                Err(TryRecvError::Empty) => {},
                                Err(err) => panic!(err),
                            }
                        }
                    },
                    SendState::Sending(buffer, _) => {
                        send_state = SendState::BackOff(buffer, BACK_OFF_WINDOW);
                    },
                    SendState::BackOff(buffer, time) => {
                        send_state = if data.len() < time {
                            SendState::BackOff(buffer, time - data.len())
                        } else {
                            if channel_free {
                                message_signal(buffer)
                            } else {
                                SendState::BackOff(buffer, BACK_OFF_WINDOW)
                            }
                        };
                    },
                    SendState::WaitAck(buffer, time) => {
                        send_state = if data.len() < time {
                            match ack_rcev_receiver.try_recv() {
                                Ok((src, dest)) => {
                                    // todo: check mac address

                                    SendState::Idle
                                },
                                Err(TryRecvError::Empty) => SendState::WaitAck(buffer, time),
                                Err(err) => panic!(err),
                            }
                        } else if channel_free {
                            message_signal(buffer)
                        } else {
                            SendState::BackOff(buffer, BACK_OFF_WINDOW)
                        };
                    },
                }

                for sample in data.iter_mut() {
                    *sample = Sample::from(&send_state.next().unwrap());
                }
            },
            |err| {
                eprintln!("an error occurred on the output audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((sender, stream))
    }

    fn create_receive_stream(
        _guard: Arc<AtomicBool>,
        _ack_send_sender: Sender<(u8, u8)>,
        _ack_rcev_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let device = get_host().default_input_device().ok_or("no input device available")?;

        let config = device.supported_input_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .min_by_key(|item| item.channels())
            .ok_or("expected configuration not found")?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut channel = 0;

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if let Some(buffer) = demodulator.push_back(Sample::from(sample)) {
                        if channel == 0 { sender.send(buffer).unwrap(); }

                        channel += 1;
                        if channel == channel_count { channel = 0; }
                    }
                }
            },
            |err| {
                eprintln!("an error occurred on the inout audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((receiver, stream))
    }

    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let guard = Arc::new(AtomicBool::new(true));

        let (ack_send_sender, ack_send_receiver) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_sender, ack_recv_receiver) = mpsc::channel::<(u8, u8)>();

        let (receiver, _output_stream) = Self::create_receive_stream(
            guard.clone(), ack_send_sender, ack_recv_sender,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            guard.clone(), ack_send_receiver, ack_recv_receiver,
        )?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: &DataPack) -> Result<(), SendError<DataPack>> {
        self.sender.send(*data)
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
