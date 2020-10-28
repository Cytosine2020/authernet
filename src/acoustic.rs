use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError},
};
use cpal::{
    Host, Device, Sample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rand::Rng;
use crate::{DataPack,
    mac::{INDEX_INDEX, MacData, MacLayer, mac_wrap},
    module::{Modulator, Demodulator},
};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 10000;
const BACK_OFF_WINDOW: usize = 512;
const IDLE_SECTION: usize = 512;


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
    BackOff(DataPack, usize, usize),
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
        guard: Arc<AtomicBool>,
        ack_send_receiver: Receiver<(u8, u8)>,
        ack_recv_receiver: Receiver<(u8, u8)>,
    ) -> Result<(Sender<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_output_configs()?)?;

        let modulator = Modulator::new();

        let channel = config.channels() as usize;

        let (sender, receiver) = mpsc::channel::<DataPack>();

        let sending = move |data| {
            SendState::Sending(data, modulator.iter(data).map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten())
        };

        let mut rng = rand::thread_rng();

        let back_off = |data, count| {
            let back_off = rng.gen_range(1, 8) + (1 << count);
            SendState::BackOff(data, back_off * BACK_OFF_WINDOW, count)
        };

        let mut send_state = SendState::Idle(0);
        let mut count = [0; 1 << MacData::MAC_SIZE];

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let mut ack_recv = ack_recv_receiver.try_iter().collect::<Vec<_>>();
                let data_len = data.len();

                for sample in data.iter_mut() {
                    let mut value = 0;

                    let channel_free = guard.load(Ordering::SeqCst);

                    match send_state {
                        SendState::Idle(time) => {
                            send_state = if time == 0 {
                                match ack_send_receiver.try_recv() {
                                    Ok((dest, index)) => {
                                        let mut buffer = mac_layer.create_ack(dest);
                                        mac_wrap(&mut buffer, index);

                                        if channel_free {
                                            sending(buffer)
                                        } else {
                                            SendState::Idle(IDLE_SECTION)
                                        }
                                    }
                                    Err(TryRecvError::Empty) => {
                                        match receiver.try_recv() {
                                            Ok(mut buffer) => {
                                                let dest = MacData::copy_from_slice(&buffer)
                                                    .get_dest();
                                                let count_ref = &mut count[dest as usize];
                                                mac_wrap(&mut buffer, *count_ref);

                                                println!("send {:?}", (dest, *count_ref));

                                                if dest != MacData::BROADCAST_MAC {
                                                    *count_ref = count_ref.wrapping_add(1);
                                                }

                                                if channel_free {
                                                    sending(buffer)
                                                } else {
                                                    back_off(buffer, 0)
                                                }
                                            }
                                            Err(TryRecvError::Empty) => {
                                                SendState::Idle(IDLE_SECTION)
                                            }
                                            Err(err) => panic!(err),
                                        }
                                    }
                                    Err(err) => panic!(err),
                                }
                            } else {
                                SendState::Idle(time - 1)
                            }
                        }
                        SendState::Sending(buffer, ref mut iter) => {
                            if channel_free {
                                if let Some(item) = iter.next() {
                                    value = item;
                                } else {
                                    let mac_data = MacData::copy_from_slice(&buffer);

                                    send_state = if mac_data.get_dest() != MacData::BROADCAST_MAC &&
                                        mac_data.get_op() != MacData::ACK {
                                        SendState::WaitAck(buffer, ACK_TIMEOUT)
                                    } else {
                                        SendState::Idle(IDLE_SECTION)
                                    }
                                }
                            } else {
                                send_state = back_off(buffer, 0)
                            }
                        }
                        SendState::BackOff(buffer, ref mut time, count) => {
                            if data_len < *time {
                                *time -= data_len;
                            } else {
                                send_state = if channel_free {
                                    sending(buffer)
                                } else {
                                    back_off(buffer, count + 1)
                                }
                            };
                        }
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
                                if channel_free {
                                    println!("retransmit");

                                    sending(buffer)
                                } else {
                                    back_off(buffer, 0)
                                }
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
        guard: Arc<AtomicBool>,
        ack_send_sender: Sender<(u8, u8)>,
        ack_recv_sender: Sender<(u8, u8)>,
    ) -> Result<(Receiver<DataPack>, cpal::Stream), Box<dyn std::error::Error>> {
        let config = select_config(device.supported_input_configs()?)?;

        let mut demodulator = Demodulator::new();

        let channel_count = config.channels() as usize;

        let (sender, receiver) = mpsc::channel();

        let mut channel = 0;
        let mut channel_active = false;

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

    pub fn new(mac_layer: MacLayer) -> Result<Self, Box<dyn std::error::Error>> {
        let channel_free = Arc::new(AtomicBool::new(false));
        let host = select_host();

        let (ack_send_sender, ack_send_receiver) = mpsc::channel::<(u8, u8)>();
        let (ack_recv_sender, ack_recv_receiver) = mpsc::channel::<(u8, u8)>();

        let (receiver, _output_stream) = Self::create_receive_stream(
            mac_layer.clone(), host.default_input_device().ok_or("no input device available!")?,
            channel_free.clone(), ack_send_sender, ack_recv_sender,
        )?;
        let (sender, _input_stream) = Self::create_send_stream(
            mac_layer, host.default_output_device().ok_or("no output device available!")?,
            channel_free.clone(), ack_send_receiver, ack_recv_receiver,
        )?;

        Ok(Self { sender, receiver, _input_stream, _output_stream })
    }

    pub fn send(&self, data: &DataPack) -> Result<(), SendError<DataPack>> {
        self.sender.send(*data)
    }

    pub fn recv(&self) -> Result<DataPack, RecvError> { self.receiver.recv() }
}
