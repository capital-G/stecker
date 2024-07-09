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
    std::unique_ptr<rust::Box<Room>> m_room;
    ~SuperStecker();
};

class SuperSteckerIn : public SuperStecker {
public:
    SuperSteckerIn();

    // ~SuperSteckerIn();
private:
    void next_k(int nSamples);
};

class SuperSteckerOut : public SuperStecker {
public:
    SuperSteckerOut();

    // ~SuperSteckerOut();

private:
    void next_k(int nSamples);
};

} // namespace SuperStecker
