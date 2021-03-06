#include <cstdio>
#include <cstring>
#include <iostream>

#if defined(__APPLE__)
#define __MACOSX_CORE__
#elif defined(__linux__)
#define __UNIX_JACK__
#else
#error "paltform not supported!"
#endif


#include "rtaudio_c.h"

#ifdef __DEBUG__
#define rtaudio_static_inline
#else
#define rtaudio_static_inline static inline __attribute__((always_inline))
#endif

#define rtaudio_unused __attribute__((unused))

rtaudio_static_inline void _warn(const char *file, int line, const char *msg) {
    fprintf(stderr, "Warn at file %s, line %d: %s\n", file, line, msg);
}

#define rtaudio_warn(msg) _warn(__FILE__, __LINE__, msg)


constexpr uint32_t CHANNEL_COUNT = 1;
constexpr uint32_t SAMPLE_FORMAT = RTAUDIO_FORMAT_SINT16;
constexpr uint32_t SAMPLE_RATE = 48000;
constexpr uint32_t BUFFER_SIZE = 16;


typedef void (*rust_callback)(void *data, int16_t *, size_t);


struct Stream {
    rtaudio_t audio;
    rust_callback inner;
    void *data;
};

rtaudio_static_inline void rtaudio_check_stream_status(rtaudio_stream_status_t status) {
    (void) status;
#ifdef __DEBUG__
    switch (status) {
        case RTAUDIO_STATUS_INPUT_OVERFLOW:
            rtaudio_warn("input overflow!");
            break;
        case RTAUDIO_STATUS_OUTPUT_UNDERFLOW:
            rtaudio_warn("output underflow!");
            break;
        default:
            break;
    }
#endif
}

int output_callback(void *out_buffer_, void *in_buffer, unsigned int size, double time,
                    rtaudio_stream_status_t status, void *userdata_) {
    (void) in_buffer;
    (void) time;
    auto *userdata = reinterpret_cast<Stream *>(userdata_);
    auto *out_buffer = reinterpret_cast<int16_t *>(out_buffer_);
    rtaudio_check_stream_status(status);

    memset(out_buffer, 0, size * sizeof(int16_t));

    userdata->inner(userdata->data, out_buffer, size);

    return 0;
}

int input_callback(void *out_buffer, void *in_buffer_, unsigned int size, double time,
                   rtaudio_stream_status_t status, void *userdata_) {
    (void) out_buffer;
    (void) time;
    auto *userdata = reinterpret_cast<Stream *>(userdata_);
    auto *in_buffer = reinterpret_cast<int16_t *>(in_buffer_);
    rtaudio_check_stream_status(status);

    userdata->inner(userdata->data, in_buffer, size);

    return 0;
}

extern "C" {
void print_device(rtaudio_device_info_t &device) {
    if (device.probed) {
        std::cout << "device: \"" << device.name << "\"";
        if (device.is_default_output) {
            std::cout << " <default output>";
        }
        if (device.is_default_input) {
            std::cout << " <default input>";
        }
        std::cout << std::endl;
        std::cout << "\tmaximum output channels: " << device.output_channels << ",\n";
        std::cout << "\tmaximum input channels: " << device.input_channels << ",\n";
        std::cout << "\tmaximum duplex channels: " << device.duplex_channels << ",\n";
        std::cout << "\tsample rate:";
        for (auto sample_rate: device.sample_rates) {
            if (sample_rate == 0) { break; }
            std::cout << ' ' << sample_rate;
        }
        std::cout << ",\n";
        std::cout << "\tpreferredSampleRate: " << device.preferred_sample_rate << ",\n";
        std::cout << "\tnativeFormats: ";

        switch (device.native_formats) {
            case RTAUDIO_FORMAT_SINT8:
                std::cout << "i8";
                break;
            case RTAUDIO_FORMAT_SINT16:
                std::cout << "i16";
                break;
            case RTAUDIO_FORMAT_SINT24:
                std::cout << "i24";
                break;
            case RTAUDIO_FORMAT_SINT32:
                std::cout << "i32";
                break;
            case RTAUDIO_FORMAT_FLOAT32:
                std::cout << "f32";
                break;
            case RTAUDIO_FORMAT_FLOAT64:
                std::cout << "f64";
                break;
            default:
                std::cout << "unknown";
        }

        std::cout << ".\n";
    }
}

rtaudio_static_inline rtaudio_t rtaudio_select_host() {
#if defined(__APPLE__)
    return rtaudio_create(RTAUDIO_API_UNSPECIFIED);
#endif

#if defined(__linux__)
    return rtaudio_create(RTAUDIO_API_UNIX_JACK);
#endif
}

rtaudio_unused void rtaudio_print_hosts() {
    rtaudio_t rtaudio = rtaudio_select_host();
    int32_t count = rtaudio_device_count(rtaudio);

    std::cout << "Host: " << rtaudio_api_display_name(rtaudio_current_api(rtaudio)) << std::endl;

    for (int32_t i = 0; i < count; ++i) {
        rtaudio_device_info_t device_info = rtaudio_get_device_info(rtaudio, i);
        print_device(device_info);
    }

    rtaudio_destroy(rtaudio);
}

void rtaudio_destroy_stream(Stream *stream) {
    rtaudio_stop_stream(stream->audio);
    rtaudio_close_stream(stream->audio);
    rtaudio_destroy(stream->audio);
    delete stream;
}

rtaudio_unused Stream *rtaudio_create_output_stream(rust_callback callback, void *data) {
    rtaudio_t rtaudio = rtaudio_select_host();
    uint32_t device = rtaudio_get_default_output_device(rtaudio);

    rtaudio_stream_parameters_t config{device, CHANNEL_COUNT, 0};

    uint32_t buffer_size = BUFFER_SIZE;

    auto *stream = new Stream{rtaudio, callback, data};

    if (rtaudio_open_stream(rtaudio, &config, nullptr, SAMPLE_FORMAT, SAMPLE_RATE, &buffer_size,
                            output_callback, stream, nullptr, nullptr)) { goto error; }

    if (rtaudio_start_stream(rtaudio)) { goto error; }

    return stream;

    error:
    std::cerr << rtaudio_error(rtaudio) << std::endl;

    rtaudio_destroy_stream(stream);

    return nullptr;
}

rtaudio_unused Stream *rtaudio_create_input_stream(rust_callback callback, void *data) {
    rtaudio_t rtaudio = rtaudio_select_host();
    uint32_t device = rtaudio_get_default_input_device(rtaudio);

    rtaudio_stream_parameters_t config{device, CHANNEL_COUNT, 0};

    uint32_t buffer_size = BUFFER_SIZE;

    auto *stream = new Stream{rtaudio, callback, data};

    if (rtaudio_open_stream(rtaudio, nullptr, &config, SAMPLE_FORMAT, SAMPLE_RATE, &buffer_size,
                            input_callback, stream, nullptr, nullptr)) { goto error; }

    if (rtaudio_start_stream(rtaudio)) { goto error; }

    return stream;

    error:
    std::cerr << rtaudio_error(rtaudio) << std::endl;

    rtaudio_destroy_stream(stream);

    return nullptr;
}
}
