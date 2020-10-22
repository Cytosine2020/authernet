use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError},
};
use cpal::{Host, Device, Sample, traits::{DeviceTrait, HostTrait, StreamTrait}};
use crate::{DataPack, mac::MacData, module::{Modulator, Demodulator}};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 256;
const BACK_OFF_WINDOW: usize = 256;


pub fn select_host() -> Host { cpal::default_host() }

pub fn select_input_device(
    host: &Host, hint: Vec<String>,
) -> Result<Device, Box<dyn std::error::Error>> {
    if hint.is_empty() {
        return Ok(host.default_input_device().ok_or("no input device available")?);
    }

    let mut devices = host.input_devices()?
        .filter_map(|item| item.name().map(|name| (item, name)).ok())
        .filter(|(_, device)| {
            hint.iter().any(|name| device.find(name).is_some())
        }).map(|(device, _)| device);

    if let Some(device) = devices.next() {
        if let Some(_) = devices.next() {
            Err("multiple input device available!")?
        } else {
            Ok(device)
        }
    } else {
        Err("no input device available!")?
    }
}

pub fn select_output_device(
    host: &Host, hint: Vec<String>,
) -> Result<Device, Box<dyn std::error::Error>> {
    if hint.is_empty() {
        return Ok(host.default_output_device().ok_or("no input device available")?);
    }

    let mut devices = host.output_devices()?
        .filter_map(|item| item.name().map(|name| (item, name)).ok())
        .filter(|(_, device)| {
            hint.iter().any(|name| device.find(name).is_some())
        }).map(|(device, _)| device);

    if let Some(device) = devices.next() {
        if let Some(_) = devices.next() {
            Err("multiple output device available!")?
        } else {
            Ok(device)
        }
    } else {
        Err("no output device available!")?
    }
}

pub fn print_config() {
    let host = select_host();

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
                *self = SendState::WaitAck(*buffer, ACK_TIMEOUT);
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
        device: Device,
        guard: Arc<AtomicBool>,
        _ack_send_receiver: Receiver<(u8, u8)>,
        ack_rcev_receiver: Receiver<(u8, u8)>,
    ) -> Result<(Sender<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
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
                                Err(TryRecvError::Empty) => {}
                                Err(err) => panic!(err),
                            }
                        }
                    }
                    SendState::Sending(buffer, _) => {
                        send_state = SendState::BackOff(buffer, BACK_OFF_WINDOW);
                    }
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
                    }
                    SendState::WaitAck(buffer, time) => {
                        send_state = if data.len() < time {
                            match ack_rcev_receiver.try_recv() {
                                Ok(mac) => {
                                    if mac == MacData::from_slice(&buffer).get_mac() {
                                        SendState::Idle
                                    } else {
                                        SendState::WaitAck(buffer, time - data.len())
                                    }
                                }
                                Err(TryRecvError::Empty) => {
                                    SendState::WaitAck(buffer, time - data.len())
                                }
                                Err(err) => panic!(err),
                            }
                        } else if channel_free {
                            message_signal(buffer)
                        } else {
                            SendState::BackOff(buffer, BACK_OFF_WINDOW)
                        };
                    }
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
        device: Device,
        guard: Arc<AtomicBool>,
        _ack_send_sender: Sender<(u8, u8)>,
        _ack_rcev_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = device.supported_input_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .min_by_key(|item| item.channels())
            .ok_or("expected configuration not found")?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut channel = 0;
        let mut channel_free = false;

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if let Some(buffer) = demodulator.push_back(Sample::from(sample)) {
                        if channel == 0 { sender.send(buffer).unwrap(); }

                        channel += 1;
                        if channel == channel_count { channel = 0; }
                    }

                    if channel_free == demodulator.active() {
                        channel_free = !demodulator.active();
                        guard.store(channel_free, Ordering::SeqCst);
                    }
                }
            },
            |err| {
                eprintln!("an error occurred on the inout audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((receiver, stream))
    }

    pub fn new(
        input: Vec<String>, output: Vec<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let guard = Arc::new(AtomicBool::new(false));
        let host = select_host();

        let (ack_send_sender, ack_send_receiver) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_sender, ack_recv_receiver) = mpsc::channel::<(u8, u8)>();

        let (receiver, _output_stream) = Self::create_receive_stream(
            select_input_device(&host, input)?, guard.clone(),
            ack_send_sender, ack_recv_sender,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            select_output_device(&host, output)?, guard.clone(),
            ack_send_receiver, ack_recv_receiver,
        )?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: &DataPack) -> Result<(), SendError<DataPack>> {
        self.sender.send(*data)
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
