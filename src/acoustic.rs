use std::sync::mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError};
use cpal::{
    Device, Sample, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crate::{
    SAMPLE_RATE, BARKER, WAVE_LENGTH,
    bit_set::DataPack,
    module::{bpsk_modulate, Wave, Modulator, Demodulator},
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

enum SenderState<I, U> {
    Message(I),
    Idle(U),
}

impl<I, U> Iterator for SenderState<I, U>
    where I: Iterator, I::Item: Into<i16>,
          U: Iterator, U::Item: Into<i16>,
{
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Message(ref mut iter) => {
                iter.next().map(|item| item.into())
            }
            Self::Idle(ref mut iter) => {
                iter.next().map(|item| item.into())
            }
        }
    }
}

pub struct AcousticSender {
    sender: Sender<DataPack>,
    _stream: cpal::Stream,
}

impl AcousticSender {
    const IDLE_SECTION: usize = WAVE_LENGTH * 8;

    fn get_device() -> Result<(Device, SupportedStreamConfig), Box<dyn std::error::Error>> {
        let device = cpal::default_host()
            .default_output_device().ok_or("no input device available")?;

        let config = device.supported_output_configs()?
            .map(|item| item.with_max_sample_rate())
            .filter(|item| item.sample_rate() == SAMPLE_RATE)
            .next().ok_or("expected configuration not found")?;

        Ok((device, config))
    }

    pub fn new(carrier: &Wave, len: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("{:?}: {:#?}", output_device.name(), &output_config);

        let modulator = Modulator::new(&carrier, len);

        let channel = config.channels() as usize;

        let channel_handler = move |item| {
            std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
        };

        let carrier_clone = carrier.clone();

        let idle_signal = move |phase, len| {
            SenderState::Idle(carrier_clone.iter(phase).take(len).map(channel_handler).flatten())
        };

        let message_signal = move |buffer| {
            SenderState::Message(modulator.iter(buffer).map(channel_handler).flatten())
        };

        let (sender, receiver) = mpsc::channel();

        let mut buffer = idle_signal(0, 8192);

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let value = buffer.next().unwrap_or_else(|| {
                        buffer = match receiver.try_recv() {
                            Ok(buffer) => message_signal(buffer),
                            Err(TryRecvError::Empty) => idle_signal(0, Self::IDLE_SECTION),
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

    pub fn new(carrier: &Wave, len: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("{:?}: {:#?}", input_device.name(), &input_config);

        let preamble = bpsk_modulate(BARKER.iter(), carrier.clone(), len)
            .collect::<Vec<_>>();

        let mut demodulator = Demodulator::new(preamble, carrier, len);

        // let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if let Some(buffer) = demodulator.push_back(Sample::from(sample)) {
                        sender.send(buffer).unwrap();
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
