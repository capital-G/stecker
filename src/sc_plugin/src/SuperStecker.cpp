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
        // @todo this does not compile on linux :/
        // ClearUnitIfMemFailed(buff);

        for (int i = 0; i < strSize; i++) {
            buff[i] = (char)in0(startIndex + i);
        }
        // terminate string
        buff[strSize] = 0;

        return rust::Str(buff, strSize);
    }

    // same as extractString but for audio rate as audio rate is using
    // float pointers instead of discrete floats
    rust::Str SuperStecker::extractStringAr(int sizeIndex, int startIndex) {
        int strSize = *in(sizeIndex);

        // +1 b/c of null termination
        const int allocSize = (strSize + 1) * sizeof(char);

        // necessary so ClearUnitIfMemFailed works
        Unit* unit = (Unit*) this;
        char* buff = (char*) RTAlloc(mWorld, allocSize);
        // @todo this does not compile on linux :/
        // ClearUnitIfMemFailed(buff);

        for (int i = 0; i < strSize; i++) {
            buff[i] = (char) *in(startIndex + i);
        }
        // terminate string
        buff[strSize] = 0;

        return rust::Str(buff, strSize);
    }

    DataStecker::~DataStecker() {
        send_data_close_signal(**m_data_room);
    }

    /*

    SuperStecker IN

    */

    DataSteckerIn::DataSteckerIn() {
        mCalcFunc = make_calc_function<DataSteckerIn, &DataSteckerIn::next_k>();

        rust::Str roomName = extractString(0, 2);
        rust::Str hostName = extractString(1, 2 + (int) in0(0));

        // smart ptr allows us to delay the initialization of room
        m_data_room = std::make_unique<rust::Box<DataRoom>>(join_data_room(
            roomName,
            hostName
        ));

        next_k(1);
    }

    void DataSteckerIn::next_k(int nSamples) {
        float msg = recv_data_message(**m_data_room);
        out0(0) = msg;
    }

    /*

    SuperStecker OUT

    */

    DataSteckerOut::DataSteckerOut() {
        mCalcFunc = make_calc_function<DataSteckerOut, &DataSteckerOut::next_k>();

        rust::Str roomName = extractString(1, 4);
        rust::Str password = extractString(2, 4 + (int) in0(1));
        rust::Str hostName = extractString(3, 4 + (int) in0(1) + (int) in0(2));

        // smart ptr allows us to delay the initialization of room
        m_data_room = std::make_unique<rust::Box<DataRoom>>(create_data_room(
            roomName,
            password,
            hostName
        ));

        next_k(1);
    }

    void DataSteckerOut::next_k(int nSamples) {
        float val = in0(0);
        float msg = send_data_message(**m_data_room, val);
        out0(0) = msg;
    }

    /*

    (Audio)SteckerOut

    */
   SteckerOut::SteckerOut() {
        mCalcFunc = make_calc_function<SteckerOut, &SteckerOut::next>();

        rust::Str roomName = extractStringAr(1, 4);
        rust::Str password = extractStringAr(2, 4 + (int) *in(1));
        rust::Str hostName = extractStringAr(3, 4 + (int) *in(1) + (int) *in(2));

        // smart ptr allows us to delay the initialization of room
        m_audio_room = std::make_unique<rust::Box<AudioRoomSender>>(create_audio_room_sender(
            roomName,
            password,
            hostName
        ));

        next(1);
   }

   void SteckerOut::next(int nSamples) {
        const float* input = in(0);
        float* outbuf = out(0);
        // @todo avoid this
        for (int i = 0; i < nSamples; ++i) {
            outbuf[i] = input[i];
        }
        push_values_to_web(**m_audio_room, outbuf, nSamples);
   }

    /*

    (Audio)SteckerIn

    */
   SteckerIn::SteckerIn() {
        mCalcFunc = make_calc_function<SteckerIn, &SteckerIn::next>();

        rust::Str roomName = extractStringAr(0, 2);
        rust::Str hostName = extractStringAr(1, 2 + (int) *in(0));

        // smart ptr allows us to delay the initialization of room
        m_audio_room = std::make_unique<rust::Box<AudioRoomReceiver>>(create_audio_room_receiver(
            roomName,
            hostName,
            mBufLength
        ));

        next(1);
   }

   void SteckerIn::next(int nSamples) {
        const float* input = in(0);
        float* outbuf = out(0);
        pull_values_from_web(**m_audio_room, outbuf, nSamples);
   }

} // namespace SuperStecker

PluginLoad(SuperSteckerUGens) {
    // Plugin magic
    ft = inTable;
    registerUnit<SuperStecker::DataSteckerIn>(ft, "DataSteckerIn", false);
    registerUnit<SuperStecker::DataSteckerOut>(ft, "DataSteckerOut", false);
    registerUnit<SuperStecker::SteckerOut>(ft, "SteckerOut", false);
    registerUnit<SuperStecker::SteckerIn>(ft, "SteckerIn", false);
}
