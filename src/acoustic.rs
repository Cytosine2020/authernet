use std::sync::{
    Arc, atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvError, TryRecvError, Sender, SendError},
};
use cpal::{
    Host, Device, Sample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rand::{thread_rng, Rng};
use crate::{
    DataPack,
    mac::{INDEX_INDEX, MacData, MacLayer, mac_wrap},
    module::{Modulator, Demodulator},
};


const SAMPLE_RATE: cpal::SampleRate = cpal::SampleRate(48000);
const ACK_TIMEOUT: usize = 10000;
const BACK_OFF_WINDOW: usize = 2000;


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


fn receiver_unwrap<T>(receiver: &Receiver<T>, flag: bool) -> Option<T> {
    if flag {
        match receiver.try_recv() {
            Ok(item) => Some(item),
            Err(TryRecvError::Empty) => None,
            Err(err) => panic!(err),
        }
    } else {
        None
    }
}


enum SendState<I> {
    Idle(Option<(DataPack, usize, usize)>),
    Sending(DataPack, I),
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

        let mut send_state = SendState::Idle(None);
        let mut mac_index = [0; 1 << MacData::MAC_SIZE];

        let data_signal = move |buffer| {
            modulator.iter(buffer).map(move |item| {
                std::iter::once(item).chain(std::iter::repeat(0).take(channel - 1))
            }).flatten()
        };

        let sending = move |buffer| {
            let mac_data = MacData::copy_from_slice(&buffer);
            let dest = mac_data.get_dest();

            println!("sending data {:?}", (dest, mac_index[dest as usize]));

            SendState::Sending(buffer, data_signal(buffer))
        };

        let back_off = move |buffer, count: usize| {
            let back_off = thread_rng().gen_range::<usize, usize, usize>(0, 16) + (1 << count);

            let dest = MacData::copy_from_slice(&buffer).get_dest();
            println!("back off {:?}", (dest, mac_index[dest as usize], back_off));

            SendState::Idle(Some((buffer, back_off * BACK_OFF_WINDOW, count)))
        };

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let mut flag = true;

                let mut ack_recv = ack_recv_receiver.try_iter().collect::<Vec<_>>();

                for sample in data.iter_mut() {
                    let mut value = 0;

                    let channel_free = guard.load(Ordering::SeqCst);

                    match send_state {
                        SendState::Idle(ref mut back) => {
                            if let Some((dest, index)) = receiver_unwrap(&ack_send_receiver, flag) {
                                let mut buffer = mac_layer.create_ack(dest);
                                mac_wrap(&mut buffer, index);

                                println!("send ack");

                                send_state = if channel_free {
                                    sending(buffer)
                                } else {
                                    println!("failed to send ack");
                                    SendState::Idle(None)
                                };
                            } else if let Some((buffer, time, count)) = back {
                                if *time == 0 {
                                    send_state = if channel_free {
                                        sending(*buffer)
                                    } else {
                                        back_off(*buffer, *count + 1)
                                    }
                                } else {
                                    *time -= 0;
                                }
                            } else if let Some(mut buffer) = receiver_unwrap(&receiver, flag) {
                                let dest = MacData::copy_from_slice(&buffer).get_dest();
                                let count_ref = &mut mac_index[dest as usize];
                                mac_wrap(&mut buffer, *count_ref);

                                if dest != MacData::BROADCAST_MAC {
                                    *count_ref = count_ref.wrapping_add(1);
                                }

                                send_state = if channel_free {
                                    sending(buffer)
                                } else {
                                    back_off(buffer, 0)
                                };
                            } else {
                                send_state = SendState::Idle(None);
                            };

                            flag = false;
                        }
                        SendState::Sending(buffer, ref mut iter) => {
                            let mac_data = MacData::copy_from_slice(&buffer);

                            if channel_free {
                                if let Some(item) = iter.next() {
                                    value = item;
                                } else {
                                    send_state = if mac_data.get_dest() != MacData::BROADCAST_MAC &&
                                        mac_data.get_op() != MacData::ACK {
                                        SendState::WaitAck(buffer, ACK_TIMEOUT)
                                    } else {
                                        SendState::Idle(None)
                                    }
                                }
                            } else {
                                send_state = if mac_data.get_op() != MacData::ACK {
                                    back_off(buffer, 0)
                                } else {
                                    SendState::Idle(None)
                                }
                            }
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
                                        SendState::Idle(None)
                                    } else {
                                        SendState::WaitAck(buffer, time - 1)
                                    }
                                }
                            } else {
                                if channel_free {
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
                                        println!("receive ack {:?}", tag);

                                        ack_recv_sender.send(tag).unwrap();
                                    }
                                    MacData::DATA => {
                                        println!("receive data {:?}", tag);

                                        ack_send_sender.send(tag).unwrap();

                                        if *count_ref == tag.1 {
                                            if tag.0 != MacData::BROADCAST_MAC {
                                                *count_ref = count_ref.wrapping_add(1);
                                            }

                                            sender.send(buffer).unwrap();
                                        }
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
        let channel_free = Arc::new(AtomicBool::new(true));
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
