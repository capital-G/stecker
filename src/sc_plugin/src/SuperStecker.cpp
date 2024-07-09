#include "SC_PlugIn.hpp"
#include "SuperStecker.hpp"
#include "rust/cxx.h"

static InterfaceTable *ft;

namespace SuperStecker
{

    /*
    
    SuperStecker IN

    */

    SuperSteckerIn::SuperSteckerIn() : m_room(nullptr) {
        mCalcFunc = make_calc_function<SuperSteckerIn, &SuperSteckerIn::next_k>();

        extractRoomName();
        // smart ptr allows us to delay the initialization of room
        m_room = std::make_unique<rust::Box<Room>>(join_room(
            extractRoomName()
        ));

        next_k(1);
    }

    rust::Str SuperSteckerIn::extractRoomName() {
        // stolen from SendReply UGen
        const int kVarOffset = 1;
        int m_roomNameSize = in0(0);

        // +1 b/c of null termination
        const int cmdRoomNameAllocSize = (m_roomNameSize + 1) * sizeof(char);
        
        char *chunk = (char *)RTAlloc(mWorld, cmdRoomNameAllocSize);
        // @todo ClearUnitIfMemFailed(chunk);
        m_roomName = chunk;

        for (int i = 0; i < (int)m_roomNameSize; i++) {
            m_roomName[i] = (char)in0(kVarOffset + i);
        }
        // terminate string
        m_roomName[m_roomNameSize] = 0;

        return rust::Str(m_roomName, m_roomNameSize);
    }

    void SuperSteckerIn::next_k(int nSamples) {
        float msg = recv_message(**m_room);
        out0(0) = msg;
    }

    /*
    
    SuperStecker OUT

    */

    SuperSteckerOut::SuperSteckerOut() : m_room(nullptr) {
        mCalcFunc = make_calc_function<SuperSteckerOut, &SuperSteckerOut::next_k>();

        extractRoomName();
        // smart ptr allows us to delay the initialization of room
        m_room = std::make_unique<rust::Box<Room>>(create_room(
            extractRoomName()
        ));

        next_k(1);
    }

    rust::Str SuperSteckerOut::extractRoomName() {
        // stolen from SendReply UGen
        const int kVarOffset = 2;
        int m_roomNameSize = in0(1);

        // +1 b/c of null termination
        const int cmdRoomNameAllocSize = (m_roomNameSize + 1) * sizeof(char);
        
        char *chunk = (char *)RTAlloc(mWorld, cmdRoomNameAllocSize);
        // @todo ClearUnitIfMemFailed(chunk);
        m_roomName = chunk;

        for (int i = 0; i < (int)m_roomNameSize; i++) {
            m_roomName[i] = (char)in0(kVarOffset + i);
        }
        // terminate string
        m_roomName[m_roomNameSize] = 0;

        return rust::Str(m_roomName, m_roomNameSize);
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
