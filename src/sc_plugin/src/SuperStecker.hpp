#pragma once

#include "SC_PlugIn.hpp"
#include "stecker_rs/lib.h"

namespace SuperStecker {

class SuperStecker : public SCUnit {
public:
    SuperStecker();

    // Destructor
    // ~{{ SuperStecker.plugin_name }}();

private:
    // Calc function
    void next_k(int nSamples);
    void extractRoomName();

    // Member variables
    int m_roomNameSize;
    char* m_roomName;
};

} // namespace SuperStecker
