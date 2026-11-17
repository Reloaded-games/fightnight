// Local conversion utility. Link against the user's installed UE SDK; never ship it with the game.
#define RADA_WRAP UERA
#include "rada_decode.h"
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <vector>
#include <stdexcept>
#include <string>
#include <cstring>

static void word(std::ostream& out, uint32_t n, int bytes) {
    for (int i = 0; i < bytes; ++i) out.put(char(n >> (8*i)));
}

// CUE4Parse concatenates streamed chunks including their UE SEEK envelopes.
// Read their two documented layouts only at block boundaries; never scan past damaged audio.
static size_t seek_table_end(const std::vector<uint8_t>& bytes, size_t at) {
    if (at+5 > bytes.size() || std::memcmp(bytes.data()+at,"SEEK",4)) return at;
    const auto mode = bytes[at+4]; size_t cursor = at+5;
    auto skip = [&](size_t count) { if (count > bytes.size()-cursor) throw std::runtime_error("Truncated SEEK table"); cursor += count; };
    auto array = [&]() {
        const size_t start = cursor; skip(4);
        uint32_t count = 0; std::memcpy(&count, bytes.data()+start, 4);
        if (count > 65536) throw std::runtime_error("SEEK table count limit");
        skip(size_t(count)*2); return count;
    };
    if (mode == 0) { skip(6); array(); }
    else if (mode == 1) { const auto count = array(); skip(4); if (array()!=count) throw std::runtime_error("SEEK table arrays differ"); }
    else throw std::runtime_error("Unknown SEEK table mode");
    return cursor;
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("Usage: decode_rada input.rada output.wav");
        const std::filesystem::path input(argv[1]), output(argv[2]);
        if (std::filesystem::exists(output)) throw std::runtime_error("Output already exists");
        const auto size = std::filesystem::file_size(input);
        if (size < 40 || size > 64*1024*1024) throw std::runtime_error("Input size limit");
        std::vector<uint8_t> bytes(size);
        std::ifstream in(input, std::ios::binary);
        if (!in.read(reinterpret_cast<char*>(bytes.data()), size)) throw std::runtime_error("Read failed");
        const auto* header = RadAGetFileHeader(bytes.data(), bytes.size());
        if (!header) throw std::runtime_error("Invalid RADA header");
        const auto channels = header->channels;
        const auto rate = RadASampleRateFromEnum(header->sample_rate);
        if (!rate || channels < 1 || channels > 8 || !header->frame_count ||
            header->frame_count > uint64_t(rate)*120 || header->file_size > size)
            throw std::runtime_error("Unsupported channels, duration or incomplete file");
        uint32_t memorySize = 0;
        if (RadAGetMemoryNeededToOpen(bytes.data(), size, &memorySize) || memorySize > 64*1024*1024)
            throw std::runtime_error("Invalid decoder memory request");
        std::vector<uint64_t> memory((memorySize+7)/8);
        auto* decoder = reinterpret_cast<RadAContainer*>(memory.data());
        if (!RadAOpenDecoder(bytes.data(), size, decoder, memorySize)) throw std::runtime_error("Decoder open failed");
        std::vector<float> block(channels*RadADecodeBlock_MaxOutputFrames);
        std::vector<int16_t> pcm;
        pcm.reserve(size_t(header->frame_count)*channels);
        size_t offset = RadAGetBytesToOpen(header), frames = 0, seekBytes = 0;
        while (frames < header->frame_count) {
            const size_t next = seek_table_end(bytes, offset); seekBytes += next-offset; offset = next;
            uint32_t needed = 0;
            if (offset >= size) throw std::runtime_error("Missing audio block");
            auto status = RadAExamineBlock(decoder, bytes.data()+offset, size-offset, &needed);
            if (status != RadAExamineBlockResult::Valid && needed > 0 && needed <= size-offset &&
                seek_table_end(bytes, offset+needed) > offset+needed)
                status = RadAExamineBlock(decoder, bytes.data()+offset, needed, &needed);
            if (status != RadAExamineBlockResult::Valid)
                throw std::runtime_error("Truncated or corrupt RADA block at byte " + std::to_string(offset) + ", frames " + std::to_string(frames) + ", needed " + std::to_string(needed));
            size_t consumed = 0;
            const int count = RadADecodeBlock(decoder, bytes.data()+offset, needed, block.data(), RadADecodeBlock_MaxOutputFrames, &consumed);
            if (count < 0 || count > RadADecodeBlock_MaxOutputFrames || !consumed || consumed > size-offset)
                throw std::runtime_error("Audio decode failed before expected frame count");
            if (frames+count > header->frame_count) throw std::runtime_error("Decoded frame count exceeds header");
            for (int i = 0; i < count; ++i) for (int c = 0; c < channels; ++c) {
                const float sample = block[c*RadADecodeBlock_MaxOutputFrames+i];
                if (!std::isfinite(sample)) throw std::runtime_error("Nonfinite PCM");
                pcm.push_back(int16_t(std::clamp(sample*32768.f, -32768.f, 32767.f)));
            }
            offset += consumed; frames += count;
        }
        if (size-seekBytes != header->file_size || offset != size) throw std::runtime_error("Unparsed bytes after audio decode");
        std::ofstream out(output, std::ios::binary);
        const uint32_t dataSize = uint32_t(pcm.size()*2);
        out.write("RIFF",4); word(out,36+dataSize,4); out.write("WAVEfmt ",8);
        word(out,16,4); word(out,1,2); word(out,channels,2); word(out,rate,4);
        word(out,rate*channels*2,4); word(out,channels*2,2); word(out,16,2);
        out.write("data",4); word(out,dataSize,4);
        out.write(reinterpret_cast<const char*>(pcm.data()), dataSize);
        if (!out) throw std::runtime_error("WAV write failed");
        std::cout << "{\"frames\":" << frames << ",\"sampleRate\":" << rate << ",\"channels\":" << int(channels) << "}\n";
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
