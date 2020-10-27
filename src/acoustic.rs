use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError},
};
use cpal::{
    Host, Device, Sample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crate::{
    DataPack,
    mac::{MacData, MacLayer},
    module::{Modulator, Demodulator},
};
use crate::mac::{INDEX_INDEX, mac_wrap};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 16384;
// const BACK_OFF_WINDOW: usize = 256;
const IDLE_SECTION: usize = 256;


pub fn select_host() -> Host { cpal::default_host() }

pub fn select_config<T: Iterator<Item=SupportedStreamConfigRange>>(
    config: T
) -> Result<SupportedStreamConfig, Box<dyn std::error::Error>> {
    Ok(config.map(|item| item.with_max_sample_rate())
        .filter(|item| item.sample_rate() == SAMPLE_RATE)
        .min_by_key(|item| item.channels())
        .ok_or("expected configuration not found")?)
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
    Idle(usize),
    Sending(DataPack, I),
    // BackOff(DataPack, usize),
    WaitAck(DataPack, usize),
}

pub struct Athernet {
    sender: Sender<DataPack>,
    receiver: Receiver<DataPack>,
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

impl Athernet {
    fn create_send_stream(
        mac_layer: MacLayer,
        device: Device,
        _guard: Arc<AtomicBool>,
        ack_send_receiver: Receiver<(u8, u8)>,
        ack_recv_receiver: Receiver<(u8, u8)>,
    ) -> Result<(Sender<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_output_configs()?)?;

        let modulator = Modulator::new();

        let channel = config.channels() as usize;

        let (sender, receiver) = mpsc::channel::<DataPack>();

        let message_signal = move |data| {
            SendState::Sending(data, modulator.iter(data).map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten())
        };

        let mut send_state = SendState::Idle(IDLE_SECTION);
        let mut count = [0; 1 << MacData::MAC_SIZE];

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let mut ack_recv = ack_recv_receiver.try_iter().collect::<Vec<_>>();

                for sample in data.iter_mut() {
                    let mut value = 0;

                    // let channel_free = guard.load(Ordering::SeqCst);

                    match send_state {
                        SendState::Idle(time) => {
                            send_state = if time == 0 {
                                // if channel_free {
                                match ack_send_receiver.try_recv() {
                                    Ok((dest, index)) => {
                                        let mut buffer = mac_layer.create_ack(dest);
                                        mac_wrap(&mut buffer, index);
                                        message_signal(buffer)
                                    }
                                    Err(TryRecvError::Empty) => {
                                        match receiver.try_recv() {
                                            Ok(mut buffer) => {
                                                let dest = MacData::copy_from_slice(&buffer).get_dest();
                                                let count_ref = &mut count[dest as usize];
                                                mac_wrap(&mut buffer, *count_ref);

                                                println!("send {:?}", (dest, *count_ref));

                                                if dest != MacData::BROADCAST_MAC {
                                                    *count_ref = count_ref.wrapping_add(1);
                                                }
                                                message_signal(buffer)
                                            }
                                            Err(TryRecvError::Empty) => {
                                                SendState::Idle(IDLE_SECTION)
                                            }
                                            Err(err) => panic!(err),
                                        }
                                    }
                                    Err(err) => panic!(err),
                                }
                                // } else {
                                //     SendState::Idle(0)
                                // }
                            } else {
                                SendState::Idle(time - 1)
                            }
                        }
                        SendState::Sending(buffer, ref mut iter) => {
                            // if channel_free {
                            if let Some(item) = iter.next() {
                                value = item;
                            } else {
                                let mac_data = MacData::copy_from_slice(&buffer);

                                send_state = if mac_data.get_dest() != MacData::BROADCAST_MAC &&
                                    mac_data.get_op() != MacData::ACK {
                                    SendState::WaitAck(buffer, ACK_TIMEOUT)
                                } else {
                                    SendState::Idle(0)
                                }
                            }
                            // } else {
                            //     send_state = SendState::BackOff(buffer, BACK_OFF_WINDOW);
                            // }
                        }
                        // SendState::BackOff(buffer, time) => {
                        //     send_state = if data.len() < time {
                        //         SendState::BackOff(buffer, time - data.len())
                        //     } else {
                        //         if channel_free {
                        //             message_signal(buffer)
                        //         } else {
                        //             SendState::BackOff(buffer, BACK_OFF_WINDOW)
                        //         }
                        //     };
                        // }
                        SendState::WaitAck(buffer, time) => {
                            send_state = if time > 0 {
                                if ack_recv.is_empty() {
                                    SendState::WaitAck(buffer, time - 1)
                                } else {
                                    let dest = MacData::copy_from_slice(&buffer).get_dest();
                                    let index = buffer[INDEX_INDEX];

                                    let result = ack_recv.iter()
                                        .any(|item| *item == (dest, index));

                                    ack_recv.clear();

                                    if result {
                                        SendState::Idle(0)
                                    } else {
                                        SendState::WaitAck(buffer, time - 1)
                                    }
                                }
                            } else {
                                // if channel_free {
                                println!("retransmit");

                                message_signal(buffer)
                                // } else {
                                //     SendState::BackOff(buffer, BACK_OFF_WINDOW)
                                // }
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
        mac_layer: MacLayer,
        device: Device,
        _guard: Arc<AtomicBool>,
        ack_send_sender: Sender<(u8, u8)>,
        ack_recv_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_input_configs()?)?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut channel = 0;
        // let mut channel_free = false;

        let mut count = [0; 1 << MacData::MAC_SIZE];

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                for sample in data.iter() {
                    if channel == 0 {
                        if let Some(buffer) = demodulator.push_back(Sample::from(sample)) {
                            if mac_layer.check(&buffer) {
                                let mac_data = MacData::copy_from_slice(&buffer);
                                let tag = (mac_data.get_src(), buffer[INDEX_INDEX]);
                                let count_ref = &mut count[tag.0 as usize];

                                match mac_data.get_op() {
                                    MacData::ACK => {
                                        ack_recv_sender.send(tag).unwrap();
                                    }
                                    MacData::DATA => {
                                        if *count_ref == tag.1 {
                                            println!("receive {:?}", tag);

                                            if tag.0 != MacData::BROADCAST_MAC {
                                                *count_ref = count_ref.wrapping_add(1);
                                            }

                                            sender.send(buffer).unwrap();
                                        }

                                        ack_send_sender.send(tag).unwrap();
                                    }
                                    _ => {}
                                }
                            } else {
                                println!("crc fail");
                            }
                        }

                        // if channel_free == demodulator.active() {
                        //     channel_free = !demodulator.active();
                        //     guard.store(channel_free, Ordering::SeqCst);
                        // }
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

    pub fn new(mac_layer: MacLayer) -> Result<Self, Box<dyn std::error::Error>> {
        let guard = Arc::new(AtomicBool::new(false));
        let host = select_host();

        let (ack_send_sender, ack_send_receiver) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_sender, ack_recv_receiver) = mpsc::channel::<(u8, u8)>();

        let (receiver, _output_stream) = Self::create_receive_stream(
            mac_layer.clone(), host.default_input_device().ok_or("no input device available!")?,
            guard.clone(), ack_send_sender, ack_recv_sender,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            mac_layer, host.default_output_device().ok_or("no output device available!")?,
            guard.clone(), ack_send_receiver, ack_recv_receiver,
        )?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: &DataPack) -> Result<(), SendError<DataPack>> {
        self.sender.send(*data)
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
