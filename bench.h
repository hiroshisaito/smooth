// smooth-mod-v1.5.0 benchmarking / regression capture
// Header-only. Enable with -DSMOOTH_BENCH=1
//
// When enabled, instruments smoothing() to:
//   - measure elapsed ms per render call
//   - dump input/output pixel buffers to /tmp/smooth_bench/frame_NNNN_{in,out}.raw
//   - append timing + parameter log to /tmp/smooth_bench/timing.log
//
// Raw file layout: 64-byte SMDP header followed by rowbytes * height pixel bytes.

#ifndef SMOOTH_BENCH_H_
#define SMOOTH_BENCH_H_

#ifdef SMOOTH_BENCH

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>

#if defined(_WIN32)
  #include <direct.h>    // _mkdir
  #define SMOOTH_BENCH_MKDIR(path) _mkdir(path)
  #define SMOOTH_BENCH_DUMP_DIR "C:\\Temp\\smooth_bench"
#else
  #include <sys/stat.h>
  #include <sys/types.h>
  #define SMOOTH_BENCH_MKDIR(path) mkdir((path), 0755)
  #define SMOOTH_BENCH_DUMP_DIR "/tmp/smooth_bench"
#endif

namespace smooth_bench {

struct DumpHeader {
    char     magic[4];             // "SMDP"
    uint32_t version;              // 1
    uint32_t width;
    uint32_t height;
    uint32_t bpc;                  // 8 or 16
    uint32_t rowbytes;             // bytes per row
    uint32_t channels;             // 4 (ARGB)
    uint32_t frame_n;
    uint32_t params_range;
    float    params_line_weight;
    uint32_t params_white;
    uint32_t reserved[5];
};
static_assert(sizeof(DumpHeader) == 64, "DumpHeader must stay 64 bytes");

inline std::atomic<uint32_t>& counter_ref() {
    static std::atomic<uint32_t> c{0};
    return c;
}

inline std::mutex& io_lock() {
    static std::mutex m;
    return m;
}

inline const char* dump_dir() { return SMOOTH_BENCH_DUMP_DIR; }

inline void ensure_dir_once() {
    static std::once_flag flag;
    std::call_once(flag, []() {
        SMOOTH_BENCH_MKDIR(dump_dir());
    });
}

inline void write_raw(const char* path, const DumpHeader& hdr,
                      const void* pixels, size_t bytes) {
    FILE* f = std::fopen(path, "wb");
    if (!f) return;
    std::fwrite(&hdr, sizeof(hdr), 1, f);
    std::fwrite(pixels, 1, bytes, f);
    std::fclose(f);
}

inline void dump_pair(uint32_t frame_n, int width, int height, int bpc, int rowbytes,
                      const void* in_pixels, const void* out_pixels,
                      uint32_t range, float line_weight, int white) {
    ensure_dir_once();
    DumpHeader hdr{};
    std::memcpy(hdr.magic, "SMDP", 4);
    hdr.version = 1;
    hdr.width = static_cast<uint32_t>(width);
    hdr.height = static_cast<uint32_t>(height);
    hdr.bpc = static_cast<uint32_t>(bpc);
    hdr.rowbytes = static_cast<uint32_t>(rowbytes);
    hdr.channels = 4;
    hdr.frame_n = frame_n;
    hdr.params_range = range;
    hdr.params_line_weight = line_weight;
    hdr.params_white = static_cast<uint32_t>(white);

    const size_t bytes = static_cast<size_t>(rowbytes) * static_cast<size_t>(height);
    char path[256];
    std::snprintf(path, sizeof(path), "%s/frame_%04u_in.raw", dump_dir(), frame_n);
    write_raw(path, hdr, in_pixels, bytes);
    std::snprintf(path, sizeof(path), "%s/frame_%04u_out.raw", dump_dir(), frame_n);
    write_raw(path, hdr, out_pixels, bytes);
}

inline void log_line(uint32_t frame_n, int width, int height, int bpc,
                     double ms, uint32_t range, float line_weight, int white) {
    std::lock_guard<std::mutex> g(io_lock());
    ensure_dir_once();
    char path[256];
    std::snprintf(path, sizeof(path), "%s/timing.log", dump_dir());
    if (FILE* f = std::fopen(path, "a")) {
        std::fprintf(f,
            "frame=%u w=%d h=%d bpc=%d range=%u lw=%.4f white=%d ms=%.3f\n",
            frame_n, width, height, bpc, range, line_weight, white, ms);
        std::fclose(f);
    }
    std::fprintf(stderr,
        "[smooth-bench] frame=%u w=%d h=%d bpc=%d ms=%.3f range=%u lw=%.3f white=%d\n",
        frame_n, width, height, bpc, ms, range, line_weight, white);
}

class Timer {
public:
    Timer() : start_(std::chrono::steady_clock::now()) {}
    double elapsed_ms() const {
        const auto now = std::chrono::steady_clock::now();
        return std::chrono::duration<double, std::milli>(now - start_).count();
    }
private:
    std::chrono::steady_clock::time_point start_;
};

} // namespace smooth_bench

#define SMOOTH_BENCH_TIMER_BEGIN() smooth_bench::Timer _smooth_bench_timer
#define SMOOTH_BENCH_CAPTURE(width, height, bpc, rowbytes, in_ptr, out_ptr, range, line_weight, white) \
    do { \
        const double _ms = _smooth_bench_timer.elapsed_ms(); \
        const uint32_t _n = smooth_bench::counter_ref().fetch_add(1); \
        smooth_bench::dump_pair(_n, (width), (height), (bpc), (rowbytes), (in_ptr), (out_ptr), \
                                (range), (line_weight), (white)); \
        smooth_bench::log_line(_n, (width), (height), (bpc), _ms, (range), (line_weight), (white)); \
    } while (0)

#else // !SMOOTH_BENCH

#define SMOOTH_BENCH_TIMER_BEGIN() ((void)0)
#define SMOOTH_BENCH_CAPTURE(...) ((void)0)

#endif // SMOOTH_BENCH

#endif // SMOOTH_BENCH_H_
