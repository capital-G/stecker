#pragma once

#include "SC_PlugIn.hpp"
#include "stecker_rs/lib.h"
#include "rust/cxx.h"

namespace SuperStecker {

class SuperStecker : public SCUnit {
public:
    SuperStecker();

    // Destructor
    // ~{{ SuperStecker.plugin_name }}();

private:
    // Calc function
    void next_k(int nSamples);
    rust::Str extractRoomName();

    // Member variables
    char* m_roomName;
    std::unique_ptr<rust::Box<Room>> m_room;
};

} // namespace SuperStecker
