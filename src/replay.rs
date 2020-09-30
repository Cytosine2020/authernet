use std::sync::mpsc::{self, TryRecvError};
use cpal::{
    Device, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const SECTION_SIZE: usize = 120000;

pub enum ChannelState<I, U> {
    Message(I),
    Idle(U),
}

impl<I, U> Iterator for ChannelState<I, U>
    where I: Iterator<Item=f32>, U: Iterator<Item=f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Message(ref mut iter) => iter.next(),
            Self::Idle(ref mut iter) => iter.next(),
        }
    }
}

fn output_device() -> Result<(Device, SupportedStreamConfig), Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_output_device().ok_or("no input device available")?;

    let config = device.supported_output_configs()?
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().ok_or("expected configuration not found")?;

    Ok((device, config))
}

fn input_device() -> Result<(Device, SupportedStreamConfig), Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_input_device().ok_or("no input device available")?;

    let config = device.supported_input_configs()?
        .map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .next().ok_or("expected configuration not found")?;

    Ok((device, config))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();

    let (output_device, output_config) = output_device()?;
    let (input_device, input_config) = input_device()?;

    println!("output {:?}: {:#?}", output_device.name(), &output_config);
    println!("input {:?}: {:#?}", input_device.name(), &input_config);

    let input_channel = input_config.channels() as usize;

    let mut input_buffer = Vec::with_capacity(SECTION_SIZE);

    let mut channel = 0;

    let input_stream = input_device.build_input_stream(
        &input_config.into(),
        move |data: &[f32], _| {
            for sample in data.iter() {
                if channel == 0 {
                    input_buffer.push(*sample);

                    if input_buffer.len() == SECTION_SIZE {
                        let mut tmp = Vec::with_capacity(SECTION_SIZE);

                        std::mem::swap(&mut tmp, &mut input_buffer);

                        sender.send(tmp).unwrap();
                    };
                }

                channel += 1;
                if channel >= input_channel { channel = 0; }
            }
        },
        |err| {
            eprintln!("an error occurred on the inout audio stream: {:?}", err);
        })?;

    input_stream.play()?;

    let output_channel = output_config.channels() as usize;

    let output_channel_handler = move |item| {
        std::iter::once(item).chain(std::iter::repeat(0.).take(output_channel - 1))
    };

    let idle_signal = move || {
        ChannelState::Idle(std::iter::repeat(0.).take(SECTION_SIZE)
            .map(output_channel_handler).flatten())
    };

    let message_signal = move |buffer: Vec<f32>| {
        ChannelState::Message(buffer.into_iter()
            .map(output_channel_handler).flatten())
    };

    let mut output_buffer = idle_signal();

    let output_stream = output_device.build_output_stream(
        &output_config.into(),
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                let value = output_buffer.next().unwrap_or_else(|| {
                    output_buffer = match receiver.try_recv() {
                        Ok(buffer) => message_signal(buffer),
                        Err(TryRecvError::Empty) => idle_signal(),
                        Err(err) => panic!(err),
                    };

                    output_buffer.next().unwrap()
                });

                *sample = value;
            }
        },
        |err| {
            eprintln!("an error occurred on the output audio stream: {:?}", err);

        })?;

    output_stream.play()?;

    std::thread::sleep(std::time::Duration::from_secs(10));

    Ok(())
}