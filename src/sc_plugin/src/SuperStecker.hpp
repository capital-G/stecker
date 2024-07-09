#pragma once

#include "SC_PlugIn.hpp"
#include "stecker_rs/lib.h"
#include "rust/cxx.h"
#include <memory>

namespace SuperStecker {

class SuperSteckerIn : public SCUnit {
public:
    SuperSteckerIn();

    // ~SuperSteckerIn();

private:
    void next_k(int nSamples);
    rust::Str extractRoomName();

    char* m_roomName;
    std::unique_ptr<rust::Box<Room>> m_room;
};

class SuperSteckerOut : public SCUnit {
public:
    SuperSteckerOut();

    // ~SuperSteckerOut();

private:
    void next_k(int nSamples);
    rust::Str extractRoomName();

    char* m_roomName;
    std::unique_ptr<rust::Box<Room>> m_room;
};

} // namespace SuperStecker
