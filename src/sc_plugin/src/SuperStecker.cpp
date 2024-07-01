#include "SC_PlugIn.hpp"
#include "SuperStecker.hpp"
#include "rust/cxx.h"

static InterfaceTable *ft;

namespace SuperStecker
{

    SuperStecker::SuperStecker() {
        mCalcFunc = make_calc_function<SuperStecker, &SuperStecker::next_k>();
        extractRoomName();
        next_k(1);
    }

    void SuperStecker::extractRoomName() {
        // stolen from SendReply UGen
        const int kVarOffset = 1;
        m_roomNameSize = in0(0);

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
    }

    void SuperStecker::next_k(int nSamples) {
        float msg = recv_message(rust::Str(m_roomName, m_roomNameSize));
        out0(0) = msg;
    }

} // namespace SuperStecker

PluginLoad(SuperSteckerUGens) {
    // Plugin magic
    ft = inTable;
    registerUnit<SuperStecker::SuperStecker>(ft, "SuperStecker", false);
}
