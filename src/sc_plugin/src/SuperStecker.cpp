#include "SC_PlugIn.hpp"
#include "SuperStecker.hpp"
#include "rust/cxx.h"

#include <iostream>

static InterfaceTable *ft;

namespace SuperStecker
{
    rust::Str SuperStecker::extractString(int sizeIndex, int startIndex) {
        int strSize = in0(sizeIndex);

        // +1 b/c of null termination
        const int allocSize = (strSize + 1) * sizeof(char);
        
        // necessary so ClearUnitIfMemFailed works
        Unit* unit = (Unit*) this;
        char* buff = (char*) RTAlloc(mWorld, allocSize);
        ClearUnitIfMemFailed(buff);

        for (int i = 0; i < strSize; i++) {
            buff[i] = (char)in0(startIndex + i);
        }
        // terminate string
        buff[strSize] = 0;

        return rust::Str(buff, strSize);
    }

    /*
    
    SuperStecker IN

    */

    SuperSteckerIn::SuperSteckerIn() {
        mCalcFunc = make_calc_function<SuperSteckerIn, &SuperSteckerIn::next_k>();

        rust::Str roomName = extractString(0, 2);
        rust::Str hostName = extractString(1, 2 + (int) in0(0));

        // smart ptr allows us to delay the initialization of room
        m_room = std::make_unique<rust::Box<Room>>(join_room(
            roomName,
            hostName
        ));

        next_k(1);
    }

    void SuperSteckerIn::next_k(int nSamples) {
        float msg = recv_message(**m_room);
        out0(0) = msg;
    }

    /*
    
    SuperStecker OUT

    */

    SuperSteckerOut::SuperSteckerOut() {
        mCalcFunc = make_calc_function<SuperSteckerOut, &SuperSteckerOut::next_k>();

        rust::Str roomName = extractString(1, 3);
        rust::Str hostName = extractString(2, 3 + (int) in0(1));

        // smart ptr allows us to delay the initialization of room
        m_room = std::make_unique<rust::Box<Room>>(create_room(
            roomName,
            hostName
        ));

        next_k(1);
    }

    void SuperSteckerOut::next_k(int nSamples) {
        float val = in0(0);
        float msg = send_message(**m_room, val);
        out0(0) = msg;
    }

} // namespace SuperStecker

PluginLoad(SuperSteckerUGens) {
    // Plugin magic
    ft = inTable;
    registerUnit<SuperStecker::SuperSteckerIn>(ft, "SuperSteckerIn", false);
    registerUnit<SuperStecker::SuperSteckerOut>(ft, "SuperSteckerOut", false);
}
