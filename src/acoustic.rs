use std::sync::mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError};
use cpal::{
    Host, Sample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crate::{ DataPack, wave::CARRIER, module::{Modulator, Demodulator} };


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);


#[cfg(target_os = "windows")]
fn get_host() -> Host {
    cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host")
}

#[cfg(target_os = "macos")]
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

pub enum ChannelState<I, U> {
    Message(I),
    Idle(U),
}

impl<I, U> Iterator for ChannelState<I, U>
    where I: Iterator<Item=i16>, U: Iterator<Item=i16>,
{
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Message(ref mut iter) => iter.next(),
            Self::Idle(ref mut iter) => iter.next(),
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
    const IDLE_SECTION: usize = 128;

    fn create_send_stream()
        -> Result<(Sender<DataPack>, cpal::Stream), Box<dyn std::error::Error>>
    {
        let device = get_host().default_output_device().ok_or("no input device available")?;

        let config = device.supported_output_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .next().ok_or("expected configuration not found")?;

        let modulator = Modulator::new(CARRIER.deep_clone());

        let channel = config.channels() as usize;

        let channel_handler = move |item| {
            std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
        };

        let idle_signal = move |len| {
            ChannelState::Idle(std::iter::repeat(0i16).take(len))
        };

        let message_signal = move |buffer| {
            ChannelState::Message(modulator.iter(buffer).map(channel_handler).flatten())
        };

        let (sender, receiver) = mpsc::channel();

        let mut buffer = idle_signal(32768);

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let value = buffer.next().unwrap_or_else(|| {
                        buffer = match receiver.try_recv() {
                            Ok(buffer) => message_signal(buffer),
                            Err(TryRecvError::Empty) => idle_signal(Self::IDLE_SECTION),
                            Err(err) => panic!(err),
                        };

                        buffer.next().unwrap()
                    });

                    *sample = Sample::from(&value);
                }
            },
            |err| {
                eprintln!("an error occurred on the output audio stream: {:?}", err);
            })?;

        stream.play()?;

        Ok((sender, stream))
    }

    fn create_receive_stream()
        -> Result<(Receiver<DataPack>, cpal::Stream), Box<dyn std::error::Error>>
    {
        let device = get_host().default_input_device().ok_or("no input device available")?;

        let config = device.supported_input_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .next().ok_or("expected configuration not found")?;

        let mut demodulator = Demodulator::new(CARRIER.deep_clone());

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
        let (sender, _input_stream) = Self::create_send_stream()?;
        let (receiver, _output_stream) = Self::create_receive_stream()?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: &DataPack) -> Result<(), SendError<DataPack>> {
        self.sender.send(*data)
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
