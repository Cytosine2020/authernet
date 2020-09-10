use cpal::{Data, Sample, SampleFormat};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};


fn write_silence<T: Sample>(data: &mut [T], _: &cpal::OutputCallbackInfo) {
    for sample in data.iter_mut() {
        *sample = Sample::from(&0.0);
    }
}

fn main() {
    let device = cpal::default_host().default_output_device().expect("no output device available");

    let supported_config = device
        .supported_output_configs().expect("error while querying configs")
        .next().expect("no supported config?!")
        .with_max_sample_rate();

//     let stream = device.build_output_stream(
//         &config,
//         move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
// // react to stream events and read or write stream data here.
//         },
//         move |err| {
// // react to errors here.
//         },
//     );

    let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();
    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(&config, write_silence::<f32>, err_fn),
        SampleFormat::I16 => device.build_output_stream(&config, write_silence::<i16>, err_fn),
        SampleFormat::U16 => device.build_output_stream(&config, write_silence::<u16>, err_fn),
    }.unwrap();

    stream.play().unwrap();
}
