#pragma once

#include "SC_PlugIn.hpp"
#include "stecker_rs/lib.h"
#include "rust/cxx.h"
#include <memory>

namespace SuperStecker {

class SuperStecker : public SCUnit {
// needs to be public so it can be accessed by subclasses
public:
    rust::Str extractString(int lenIndex, int startIndex);
    rust::Str extractStringAr(int lenIndex, int startIndex);
};

class DataStecker : public SuperStecker {
public:
    std::unique_ptr<rust::Box<DataRoom>> m_data_room;
    ~DataStecker();
};

class DataSteckerIn : public DataStecker {
public:
    DataSteckerIn();

private:
    void next_k(int nSamples);
};

class DataSteckerOut : public DataStecker {
public:
    DataSteckerOut();

private:
    void next_k(int nSamples);
};

class AudioStecker : public SuperStecker {
public:
    std::unique_ptr<rust::Box<AudioRoomSender>> m_audio_room;
};

class SteckerOut : public AudioStecker {
public:
    SteckerOut();

private:
    void next(int nSamples);
};

} // namespace SuperStecker
