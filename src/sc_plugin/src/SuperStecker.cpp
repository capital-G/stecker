#include "SC_PlugIn.hpp"
#include "SuperStecker.hpp"
#include "rust/cxx.h"

#include <iostream>

static InterfaceTable *ft;

rust::Str extractString(Unit* unit, World* world, int lenIndex, int startIndex) {
    auto strSize = static_cast<size_t>(unit->mInBuf[lenIndex][0]);

    // +1 b/c of null termination
    auto allocSize = static_cast<int>((strSize + 1) * sizeof(char));

    // necessary so ClearUnitIfMemFailed works
    // Unit* unit = (Unit*) this;
    auto* buff = static_cast<char*>(RTAlloc(world, allocSize));
    // @todo this does not compile on linux :/
    // ClearUnitIfMemFailed(buff);

    for (int i = 0; i < strSize; i++) {
        buff[i] = static_cast<char>(unit->mInBuf[startIndex + i][0]);
    }
    // terminate string
    buff[strSize] = 0;

    return {buff, strSize};
}

rust::Str extractStringAr(Unit* unit, World* world, int lenIndex, int startIndex) {
    auto strSize = static_cast<size_t>(unit->mInBuf[lenIndex][0]);

    // +1 b/c of null termination
    auto allocSize = static_cast<int>((strSize + 1) * sizeof(char));

    // necessary so ClearUnitIfMemFailed works
    char* buff = (char*) RTAlloc(world, allocSize);
    // @todo this does not compile on linux :/
    // ClearUnitIfMemFailed(buff);

    for (int i = 0; i < strSize; i++) {
        buff[i] = static_cast<char>(unit->mInBuf[startIndex + i][0]);
    }
    // terminate string
    buff[strSize] = 0;

    return {buff, strSize};
}

/*

SuperStecker IN

*/

DataSteckerReceiver::DataSteckerReceiver():mDataRoom(nullptr) {
    mCalcFunc = make_calc_function<DataSteckerReceiver, &DataSteckerReceiver::next_k>();

    rust::Str roomName = extractString(this, mWorld, 0, 2);
    rust::Str hostName = extractString(this, mWorld, 1, 2 + static_cast<int>(in0(0)));

    mDataRoom = join_data_room(
        roomName,
        hostName
    );

    next_k(1);
}

void DataSteckerReceiver::next_k(int nSamples) {
    out0(0) = recv_data_message(*mDataRoom);
}


/*

SuperStecker OUT

*/

DataSteckerSender::DataSteckerSender():mDataRoom(nullptr) {
    mCalcFunc = make_calc_function<DataSteckerSender, &DataSteckerSender::next_k>();

    rust::Str roomName = extractString(this, mWorld , 1, 4);
    rust::Str password = extractString(this, mWorld, 2, 4 + (int) in0(1));
    rust::Str hostName = extractString(this, mWorld, 3, 4 + (int) in0(1) + (int) in0(2));

    // smart ptr allows us to delay the initialization of room
    mDataRoom = create_data_room(
        roomName,
        password,
        hostName
    );

    next_k(1);
}

void DataSteckerSender::next_k(int nSamples) {
    float val = in0(0);
    send_data_message(mDataRoom, val);
    out0(0) = val;
}

/*

(Audio)SteckerOut

*/
SteckerOut::SteckerOut() {
    mCalcFunc = make_calc_function<SteckerOut, &SteckerOut::next>();

    rust::Str roomName = extractStringAr(this, mWorld, 1, 4);
    rust::Str password = extractStringAr(this, mWorld, 2, 4 + (int) *in(1));
    rust::Str hostName = extractStringAr(this, mWorld, 3, 4 + (int) *in(1) + (int) *in(2));

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

    rust::Str roomName = extractStringAr(this, mWorld, 0, 2);
    rust::Str hostName = extractStringAr(this, mWorld, 1, 2 + (int) *in(0));

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

PluginLoad(SuperSteckerUGens) {
    ft = inTable;
    registerUnit<DataSteckerReceiver>(ft, "DataSteckerIn", false);
    registerUnit<DataSteckerSender>(ft, "DataSteckerOut", false);
    registerUnit<SteckerOut>(ft, "SteckerOut", false);
    registerUnit<SteckerIn>(ft, "SteckerIn", false);
}
