#include <iostream>
#include <cstdlib>

#include "RtAudio.h"


extern "C" {
RtAudio rtaudio{};

void print_device(RtAudio::DeviceInfo &device) {
    if (device.probed) {
        // Print, for example, the maximum number of output channels for each device
        std::cout << "device " << ": " << device.name << ",\n";
        std::cout << "maximum output channels: " << device.outputChannels << ",\n";
        std::cout << "maximum input channels: " << device.inputChannels << ",\n";
        std::cout << "maximum duplex channels: " << device.duplexChannels << ",\n";
        std::cout << "sample rate:";
        for (auto sample_rate: device.sampleRates) {
            std::cout << ' ' << sample_rate;
        }
        std::cout << ",\n";
        std::cout << "preferredSampleRate: " << device.preferredSampleRate << ",\n";
        std::cout << "nativeFormats: ";

        switch (device.nativeFormats) {
            case RTAUDIO_SINT8:
                std::cout << "i8";
                break;
            case RTAUDIO_SINT16:
                std::cout << "i16";
                break;
            case RTAUDIO_SINT24:
                std::cout << "i24";
                break;
            case RTAUDIO_SINT32:
                std::cout << "i32";
                break;
            case RTAUDIO_FLOAT32:
                std::cout << "f32";
                break;
            case RTAUDIO_FLOAT64:
                std::cout << "f64";
                break;
            default:
                std::cout << "unknown";
        }

        std::cout << ".\n";
    }
}

unsigned select_default_input() { return rtaudio.getDefaultInputDevice(); }

unsigned select_default_output() { return rtaudio.getDefaultOutputDevice(); }

void rtaudio_print_hosts() {
    RtAudio::DeviceInfo input = rtaudio.getDeviceInfo(select_default_input());
    RtAudio::DeviceInfo output = rtaudio.getDeviceInfo(select_default_output());

    print_device(input);
    print_device(output);
}

// Pass-through function.
int inout(void *outputBuffer, void *inputBuffer, unsigned int nBufferFrames, double streamTime,
          RtAudioStreamStatus status, void *data) {
    (void) nBufferFrames;
    (void) streamTime;

    // Since the number of input and output channels is equal, we can do
    // a simple buffer copy operation here.
    if (status) std::cout << "Stream over/underflow detected." << std::endl;
    auto bytes = (unsigned long *) data;
    memcpy(outputBuffer, inputBuffer, *bytes);
    return 0;
}

void main_() {
    rtaudio_print_hosts();

    if (rtaudio.getDeviceCount() < 1) {
        std::cout << "\nNo audio devices found!\n";
        exit(0);
    }
    // Set the same number of channels for both input and output.
    uint32_t bufferFrames = 512;
    uint32_t bufferBytes = bufferFrames * sizeof(int16_t);
    RtAudio::StreamParameters iParams, oParams;
    iParams.deviceId = select_default_input();
    iParams.nChannels = 1;
    oParams.deviceId = select_default_output();
    oParams.nChannels = 1;

    try {
        rtaudio.openStream(&oParams, &iParams, RTAUDIO_SINT16, 48000, &bufferFrames, &inout,
                           (void *) &bufferBytes);
    } catch (RtAudioError &e) {
        e.printMessage();
        exit(0);
    }

    try {
        rtaudio.startStream();
        char input;
        std::cout << "\nRunning ... press <enter> to quit.\n";
        std::cin.get(input);
        // Stop the stream.
        rtaudio.stopStream();
    } catch (RtAudioError &e) {
        e.printMessage();
        goto cleanup;
    }

    cleanup:
    if (rtaudio.isStreamOpen()) {
        rtaudio.closeStream();
    }
}
}