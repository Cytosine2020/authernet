use std::sync::mpsc::{Receiver, RecvError, TryRecvError, Sender, SendError};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SupportedStreamConfig,
};
use crate::{
    SAMPLE_RATE, BARKER, WAVE_LENGTH,
    wave::Wave,
    bit_set::DataPack,
    module::{assemble_data_pack, Demodulator},
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
                match iter.next() {
                    Some(item) => Some(item.into()),
                    None => None,
                }
            }
            Self::Idle(ref mut iter) => {
                match iter.next() {
                    Some(item) => Some(item.into()),
                    None => None,
                }
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

    pub fn new(carrier: Wave, len: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("{:?}: {:#?}", output_device.name(), &output_config);

        let channel = config.channels() as usize;

        let channel_handler = move |item| {
            std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
        };

        let idle_signal = move |carrier: Wave, phase, len| {
            SenderState::Idle(carrier.iter(phase).take(len).map(channel_handler).flatten())
        };

        let message_signal = move |buffer, carrier, len| {
            SenderState::Message(assemble_data_pack(buffer, carrier, len).map(channel_handler).flatten())
        };

        let (sender, receiver) = std::sync::mpsc::channel();

        let mut output_buffer = idle_signal(carrier, 0, 8192);

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let value = if let Some(item) = output_buffer.next() {
                        item
                    } else {
                        output_buffer = match receiver.try_recv() {
                            Ok(buffer) => message_signal(buffer, carrier, len),
                            Err(TryRecvError::Empty) => idle_signal(carrier, 0, Self::IDLE_SECTION),
                            Err(err) => panic!(err),
                        };

                        output_buffer.next().unwrap()
                    };

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

    pub fn new(carrier: Wave, len: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let (device, config) = Self::get_device()?;

        // println!("{:?}: {:#?}", input_device.name(), &input_config);

        // let channel_count = config.channels() as usize;

        let (sender, receiver) = std::sync::mpsc::channel();

        let mut demodulator = Demodulator::new(BARKER.iter(), carrier, len);

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
