#include <iostream>
#include <cstdlib>

#include "RtAudio.h"
#include "rtaudio_c.h"


#define rtaudio_static_inline static inline __attribute__((always_inline))

rtaudio_static_inline void _warn(const char *file, int line, const char *msg) {
    std::cerr << "Warn at file " << file << ", line " << line << ": " << msg << std::endl;
}

#define rtaudio_warn(msg) _warn(__FILE__, __LINE__, msg)


constexpr uint32_t CHANNEL_COUNT = 1;
constexpr uint32_t SAMPLE_FORMAT = RTAUDIO_FORMAT_SINT16;
constexpr uint32_t SAMPLE_RATE = 48000;
constexpr uint32_t BUFFER_SIZE = 128;


typedef void (*rust_callback)(void *data, int16_t *, size_t);

struct CallbackData {
    rust_callback inner;
    void *data;
};

struct Stream {
    rtaudio_t audio;
    CallbackData *data;
};

rtaudio_static_inline void rtaudio_check_stream_status(rtaudio_stream_status_t status) {
    switch (status) {
        case RTAUDIO_STATUS_INPUT_OVERFLOW:
            rtaudio_warn("input overflow!");
            break;
        case RTAUDIO_STATUS_OUTPUT_UNDERFLOW:
            rtaudio_warn("output overflow!");
            break;
        default:
            break;
    }
}

int output_callback(void *out_buffer_, void *in_buffer, unsigned int size, double time,
                    rtaudio_stream_status_t status, void *userdata_) {
    (void) in_buffer;
    (void) time;
    auto *userdata = reinterpret_cast<CallbackData *>(userdata_);
    auto *out_buffer = reinterpret_cast<int16_t *>(out_buffer_);
    rtaudio_check_stream_status(status);

    userdata->inner(userdata->data, out_buffer, size);

    return 0;
}

int input_callback(void *out_buffer, void *in_buffer_, unsigned int size, double time,
                   rtaudio_stream_status_t status, void *userdata_) {
    (void) out_buffer;
    (void) time;
    auto *userdata = reinterpret_cast<CallbackData *>(userdata_);
    auto *in_buffer = reinterpret_cast<int16_t *>(in_buffer_);
    rtaudio_check_stream_status(status);

    userdata->inner(userdata->data, in_buffer, size);

    return 0;
}

extern "C" {
void print_device(rtaudio_device_info_t &device) {
    if (device.probed) {
        // Print, for example, the maximum number of output channels for each device
        std::cout << "device: " << device.name << ",\n";
        std::cout << "maximum output channels: " << device.output_channels << ",\n";
        std::cout << "maximum input channels: " << device.input_channels << ",\n";
        std::cout << "maximum duplex channels: " << device.duplex_channels << ",\n";
        std::cout << "sample rate:";
        for (auto sample_rate: device.sample_rates) {
            if (sample_rate == 0) { break; }
            std::cout << ' ' << sample_rate;
        }
        std::cout << ",\n";
        std::cout << "preferredSampleRate: " << device.preferred_sample_rate << ",\n";
        std::cout << "nativeFormats: ";

        switch (device.native_formats) {
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

rtaudio_static_inline rtaudio_t rtaudio_select_host() {
    return rtaudio_create(RTAUDIO_API_UNSPECIFIED);
}

rtaudio_static_inline int32_t rtaudio_select_default_input(rtaudio_t host) {
    return rtaudio_get_default_input_device(host);
}

rtaudio_static_inline int32_t rtaudio_select_default_output(rtaudio_t host) {
    return rtaudio_get_default_output_device(host);
}

void rtaudio_print_hosts() {
    rtaudio_t rtaudio = rtaudio_select_host();
    rtaudio_device_info_t output = rtaudio_get_device_info(rtaudio,
                                                           rtaudio_select_default_output(rtaudio));
    rtaudio_device_info_t input = rtaudio_get_device_info(rtaudio,
                                                          rtaudio_select_default_input(rtaudio));

    print_device(input);
    print_device(output);
}

Stream *rtaudio_create_output_stream(rust_callback callback, void *data) {
    rtaudio_t rtaudio = rtaudio_select_host();
    uint32_t device = rtaudio_select_default_output(rtaudio);

    rtaudio_stream_parameters_t config{
            .device_id = device,
            .num_channels = CHANNEL_COUNT,
            .first_channel = 0
    };

    uint32_t buffer_size = BUFFER_SIZE;

    auto *callback_data = new CallbackData{.inner = callback, .data = data};

    if (rtaudio_open_stream(rtaudio, &config, nullptr, SAMPLE_FORMAT, SAMPLE_RATE, &buffer_size,
                            output_callback, callback_data, nullptr, nullptr)) { goto error; }

    if (buffer_size != BUFFER_SIZE) {
        std::stringstream buffer;
        buffer << "output buffer size: " << buffer_size;
        rtaudio_warn(buffer.str().c_str());
    }

    if (rtaudio_start_stream(rtaudio)) {
        rtaudio_close_stream(rtaudio);
        goto error;
    }

    return new Stream{rtaudio, callback_data};

    error:
    std::cerr << rtaudio_error(rtaudio) << std::endl;

    rtaudio_destroy(rtaudio);

    return nullptr;
}

Stream *rtaudio_create_input_stream(rust_callback callback, void *data) {
    rtaudio_t rtaudio = rtaudio_select_host();
    uint32_t device = rtaudio_select_default_input(rtaudio);

    rtaudio_stream_parameters_t config{
            .device_id = device,
            .num_channels = CHANNEL_COUNT,
            .first_channel = 0
    };

    uint32_t buffer_size = BUFFER_SIZE;

    auto *callback_data = new CallbackData{.inner = callback, .data = data};

    if (rtaudio_open_stream(rtaudio, nullptr, &config, SAMPLE_FORMAT, SAMPLE_RATE, &buffer_size,
                            input_callback, callback_data, nullptr, nullptr)) { goto error; }

    if (buffer_size != BUFFER_SIZE) {
        std::stringstream buffer;
        buffer << "input buffer size: " << buffer_size;
        rtaudio_warn(buffer.str().c_str());
    }

    if (rtaudio_start_stream(rtaudio)) {
        rtaudio_close_stream(rtaudio);
        goto error;
    }

    return new Stream{rtaudio, callback_data};

    error:
    std::cerr << rtaudio_error(rtaudio) << std::endl;

    rtaudio_destroy(rtaudio);

    return nullptr;
}

void rtaudio_destroy_stream(Stream *stream) {
    rtaudio_stop_stream(stream->audio);
    rtaudio_close_stream(stream->audio);
    rtaudio_destroy(stream->audio);
    delete stream->data;
    delete stream;
}
}