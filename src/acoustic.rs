use std::sync::mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError};
use cpal::{
    Device, Sample, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crate::{
    SAMPLE_RATE, CHANNEL,
    wave::{Wave, Synthesizer},
    bit_set::DataPack, module::{Modulator, Demodulator}
};


pub fn print_hosts() {
    for host in cpal::available_hosts().into_iter()
        .filter_map(|item| cpal::host_from_id(item).ok()) {
        println!("Host: {:?}", host.id());

        for device in host.devices().into_iter().flatten() {
            println!("Device name: {:?}", device.name());

            for config in device
                .supported_output_configs().expect("error while querying configs")
                .map(|item| item.with_max_sample_rate()) {
                println!("output: {:#?}", config);
            }

            for config in device
                .supported_input_configs().expect("error while querying configs")
                .map(|item| item.with_max_sample_rate()) {
                println!("input: {:#?}", config);
            }
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

pub struct AcousticSender {
    sender: Sender<DataPack>,
    _stream: cpal::Stream,
}

impl AcousticSender {
    const IDLE_SECTION: usize = 128;

    fn get_device() -> Result<(Device, SupportedStreamConfig), Box<dyn std::error::Error>> {
        let device = cpal::default_host()
            .default_output_device().ok_or("no input device available")?;

        let config = device.supported_output_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .next().ok_or("expected configuration not found")?;

        Ok((device, config))
    }

    pub fn new(carrier: &Wave) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("output {:?}: {:#?}", device.name(), &config);

        let modulator = Modulator::new(carrier.deep_clone());

        let channel = config.channels() as usize;

        let channel_handler = move |item| {
            std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
        };

        let carrier_clone = carrier.deep_clone();

        let idle_signal = move |len| {
            let synthesizer = Synthesizer::new(
                (0..CHANNEL).map(|i| carrier_clone.iter(i, 0))
            );

            ChannelState::Idle(synthesizer.take(len).map(channel_handler).flatten())
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

        Ok(Self { sender, _stream: stream })
    }

    pub fn send(&self, data: DataPack) -> Result<(), SendError<DataPack>> { self.sender.send(data) }
}

pub struct AcousticReceiver {
    receiver: Receiver<DataPack>,
    _stream: cpal::Stream,
}

impl AcousticReceiver {
    fn get_device() -> Result<(Device, SupportedStreamConfig), Box<dyn std::error::Error>> {
        let device = cpal::default_host()
            .default_input_device().ok_or("no input device available")?;

        let config = device.supported_input_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .next().ok_or("expected configuration not found")?;

        Ok((device, config))
    }

    pub fn new(carrier: &Wave) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("input {:?}: {:#?}", device.name(), &config);

        let mut demodulator = Demodulator::new(carrier.deep_clone());

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

        Ok(Self { receiver, _stream: stream })
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
