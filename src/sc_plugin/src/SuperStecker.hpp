#pragma once

#include "SC_PlugIn.hpp"
#include "stecker_rs/lib.h"
#include "rust/cxx.h"
#include <memory>


class DataSteckerReceiver : public SCUnit {
public:

    DataSteckerReceiver();
    ~DataSteckerReceiver() {
        close_data_receiver_room(mDataRoom);
    }
    void next_k(int);

    DataRoomReceiver *mDataRoom;
};

class DataSteckerSender : public SCUnit {
public:
    DataSteckerSender();
    ~DataSteckerSender() {
        close_data_sender_room(mDataRoom);
    };

private:
    void next_k(int nSamples);
    DataRoomSender *mDataRoom;
};

class SteckerOut : public SCUnit {
public:
    std::unique_ptr<rust::Box<AudioRoomSender>> m_audio_room;
    SteckerOut();

private:
    void next(int nSamples);
};

class SteckerIn : public SCUnit {
public:
    std::unique_ptr<rust::Box<AudioRoomReceiver>> m_audio_room;
    SteckerIn();

private:
    void next(int nSamples);
};
